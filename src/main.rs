#[cfg(feature = "server")]
use axum::{Json, extract::State, http::StatusCode, routing::post};
use k8s_openapi::{
    Resource as _, api::core::v1::Pod, apimachinery::pkg::apis::meta::v1::OwnerReference,
};
use kube::{
    Api, Client,
    api::{DynamicObject, ObjectMeta, Patch, PatchParams},
    discovery::ApiResource,
    error::ErrorResponse,
};
use snafu::{Whatever, prelude::*};
use std::{env, io::Write, mem};
#[cfg(feature = "server")]
use tokio::signal;

fn find_controller(meta: &mut ObjectMeta) -> Option<&mut OwnerReference> {
    meta.owner_references
        .as_mut()
        .and_then(|owners| owners.iter_mut().find(|o| Some(true) == o.controller))
}

const MANAGER: &'static str = "podcounter.2krueger.de";
const CONTROLLER_ANNOTATION: &'static str = "podcounter.2krueger.de/controlled-pods";
const NUMBER_ANNOTATION: &'static str = "podcounter.2krueger.de/pod-number";

async fn find_outer_controller(
    mut client: Client,
    mut meta: ObjectMeta,
) -> Result<(ApiResource, Api<DynamicObject>, ObjectMeta), Whatever> {
    let mut controller = find_controller(&mut meta)
        .whatever_context("only controller managed pods are supported")?;
    loop {
        let controller_name = mem::take(&mut controller.name);
        let api_resource = ApiResource::from_gvk(&mem::take(controller).into());
        let api: Api<DynamicObject> =
            Api::namespaced_with(client, &meta.namespace.unwrap_or_default(), &api_resource);
        meta = api
            .get_metadata(&controller_name)
            .await
            .whatever_context("Failed to find object")?
            .metadata;
        if let Some(outer_controller) = find_controller(&mut meta) {
            controller = outer_controller;
            client = api.into();
        } else {
            break Ok((api_resource, api, meta));
        }
    }
}

async fn increase_controller_annotation(
    api_resource: ApiResource,
    api: &Api<DynamicObject>,
    mut meta: ObjectMeta,
) -> Result<u64, Whatever> {
    loop {
        let resource_version = meta.resource_version;

        let pod_count_annotation_value: u64 = meta
            .annotations
            .as_ref()
            .and_then(|map| map.get(CONTROLLER_ANNOTATION))
            .and_then(|value| value.parse().ok())
            .unwrap_or(0);

        let new_annotation = serde_json::json!({
            "apiVersion": &api_resource.api_version,
            "kind": &api_resource.kind,
            "metadata": {
                "resourceVersion": resource_version,
                "annotations": {
                    CONTROLLER_ANNOTATION: (pod_count_annotation_value + 1).to_string(),
                },
            },
        });
        let result = api
            .patch_metadata(
                meta.name.as_ref().unwrap(),
                &PatchParams::apply(MANAGER).force(),
                &Patch::Apply(&new_annotation),
            )
            .await;
        match result {
            Ok(_) => break Ok(pod_count_annotation_value),
            Err(kube::Error::Api(ErrorResponse { code: 409, .. })) => {
                if let Some(version) = resource_version {
                    eprintln!(
                        "Conflict while updating Controller with resourceVersion {version}. Retrying."
                    );
                }
                meta = api
                    .get_metadata(meta.name.as_ref().unwrap())
                    .await
                    .whatever_context("Failed to retrieve controller")?
                    .metadata;
            }
            Err(err) => return Err(err).whatever_context("Failed to update resource"),
        }
    }
}

fn get_pod_number_annotation(meta: &ObjectMeta) -> Option<u64> {
    meta.annotations
        .as_ref()
        .and_then(|map| map.get(NUMBER_ANNOTATION))
        .and_then(|value| value.parse().ok())
}

async fn get_or_assign_pod_number(
    client: Client,
    pod_name: &str,
    namespace: &str,
) -> Result<u64, Whatever> {
    let pod_api: Api<Pod> = Api::namespaced(client, namespace);

    let mut pod_meta = pod_api
        .get_metadata(pod_name)
        .await
        .whatever_context("Failed to read metadata")?
        .metadata;

    if let Some(pod_number) = get_pod_number_annotation(&pod_meta) {
        return Ok(pod_number);
    }

    let mut pod_resource_version = mem::take(&mut pod_meta.resource_version);

    let (api_resource, api, controller_meta) = find_outer_controller(pod_api.into(), pod_meta)
        .await
        .whatever_context("Failed to determine controller")?;

    let pod_number = increase_controller_annotation(api_resource, &api, controller_meta)
        .await
        .whatever_context("Failed to increase controller pod count annotation")?;

    let pod_api: Api<Pod> = Api::namespaced(api.into(), namespace);

    loop {
        let new_annotation = serde_json::json!({
            "apiVersion": Pod::API_VERSION,
            "kind": Pod::KIND,
            "metadata": {
                "resourceVersion": pod_resource_version,
                "annotations": {
                    NUMBER_ANNOTATION: pod_number.to_string(),
                },
            },
        });
        let result = pod_api
            .patch_metadata(
                pod_name,
                &PatchParams::apply(MANAGER).force(),
                &Patch::Apply(&new_annotation),
            )
            .await;
        match result {
            Ok(_) => break Ok(pod_number),
            Err(kube::Error::Api(ErrorResponse { code: 409, .. })) => {
                if let Some(resource_version) = pod_resource_version {
                    eprintln!(
                        "Conflict while updating Pod with resourceVersion {resource_version}. Retrying."
                    );
                }
                pod_meta = pod_api
                    .get_metadata(pod_name)
                    .await
                    .whatever_context("Failed to retrieve pod")?
                    .metadata;

                if let Some(pod_number) = get_pod_number_annotation(&pod_meta) {
                    return Ok(pod_number);
                }

                pod_resource_version = pod_meta.resource_version;
            }
            Err(err) => return Err(err).whatever_context("Failed to update resource"),
        }
    }
}

#[cfg(feature = "server")]
#[derive(serde::Deserialize)]
struct Request {
    pod_name: String,
    namespace: Option<String>,
}

#[cfg(feature = "server")]
#[derive(serde::Serialize)]
struct Response {
    pod_number: u64,
}

#[cfg(feature = "server")]
async fn get_handler(
    State(client): State<Client>,
    Json(request): Json<Request>,
) -> Result<Json<Response>, StatusCode> {
    let namespace = request
        .namespace
        .unwrap_or_else(|| client.default_namespace().to_owned());
    match get_or_assign_pod_number(client, &request.pod_name, &namespace).await {
        Ok(pod_number) => Ok(Json(Response { pod_number })),
        Err(err) => {
            eprintln!("{}", snafu::Report::from_error(err));
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

#[snafu::report]
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Whatever> {
    let mut args = env::args();
    let _ = args.next().whatever_context("argv0 requried")?;
    let output_path = args.next();
    if let Some(output_path) = output_path {
        let pod_name = args.next().whatever_context("pod name required")?;
        let namespace = args.next();
        mem::drop(args);

        let client = Client::try_default()
            .await
            .whatever_context("failed to load k8s credentials")?;
        let namespace = namespace.unwrap_or_else(|| client.default_namespace().to_owned());
        let pod_number = get_or_assign_pod_number(client, &pod_name, &namespace).await?;

        println!("Assigned pod number: {pod_number}");
        let mut file =
            std::fs::File::create(output_path).whatever_context("failed to open output file")?;
        writeln!(file, "POD_NUMBER={pod_number}")
            .whatever_context("Failed to write output file")?;
    } else {
        #[cfg(feature = "server")]
        {
            let client = Client::try_default()
                .await
                .whatever_context("failed to load k8s credentials")?;
            let app = axum::Router::new()
                .route("/", post(get_handler))
                .with_state(client);
            let listener = tokio::net::TcpListener::bind("[::]:3000")
                .await
                .whatever_context("Failed to bind to port")?;
            axum::serve(listener, app)
                .with_graceful_shutdown(shutdown_signal())
                .await
                .whatever_context("Server failed")?;
        }
    }

    Ok(())
}

#[cfg(feature = "server")]
async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}

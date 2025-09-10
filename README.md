Sequential numbering for k8s pods
=================================

In Kubernetes deployments multiple stateless pod are started. When generating e.g. Snowflake IDs a short 10 bit machine ID is needed to uniquely differentiate all pods running at the same time.

Normally Kubernetes pods can be identified through their UID or the pod name, but since these are effectively randomly distributed, 10 bits are too little to uniqueness once multiple pods are started. (E.g. choosing random 10 bit identifiers on 10 pods has a roughly 4% collision probability)

To avoid this, this project provides consecutive IDs to all pods managed by the same provider. A small service gets provided which can be called with a pod and namespace name and then provides a 64 bit unsigned ID uniquely identifying the pod inside of the deployment (or daemonset or other controller). Since the numbers are consecutive a n bit suffix can be extracted withough causing conflicts (unless more than 2^n pods are running, but then having n-bit identifiers is bound to fail anyway).

The service is very resource friendly, but if you prefer not running services it can also be started a s a init container. This is not recommended for security reasons though (the init container needs to annotate the pod and deployment definitions, so the service account of the pod would need a lot of k8s access).

Installation
------------

Run

    kubectl apply -k https://github.com/zauguin/podcounter.git//k8s/

Usage
-----

Afterwards the identifier of a pod can be queried inside the cluster by

    curl -v -H 'Content-Type: application/json' -d '{"pod_name": "<PODNAME>", "namespace": "<NAMESPACE NAME>"}' podcounter.default.svc

The namespace can be omitted, then pods in the same namespace as podcounter are assumed.
The response has the structure

    {"pod_number":0}

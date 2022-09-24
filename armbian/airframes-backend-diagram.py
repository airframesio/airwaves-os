#!/usr/bin/env python3

from diagrams import Cluster, Diagram
from diagrams.onprem.database import PostgreSQL
from diagrams.onprem.inmemory import Redis
from diagrams.onprem.compute import Server
from diagrams.onprem.network import Nginx
from diagrams.onprem.monitoring import Grafana, Prometheus
from diagrams.onprem.queue import Nats, ZeroMQ

with Diagram("Airframes Backend Architecture", show=False):
  ingest_ingress = Nginx("Ingest Ingress")
  api_ingress = Nginx("API Ingress")

  metrics = Prometheus("metric")
  metrics << Grafana("monitoring")

  with Cluster("Ingest Cluster"):
    ingest_cluster = [
      Server("acars-acarsdec"),
      Server("hfdl-dumphfdl"),
      Server("vdl-dumpvdl2"),
      Server("vdl-vdlm2dec"),
      Server("satcom-aoa"),
      Server("satcom-aoi"),
      ZeroMQ("vdl-dumpvdl2-zmq")
    ]

  with Cluster("API Cluster"):
    api_cluster = [
      Server("api1"),
      Server("api2"),
      Server("api3")
    ]

  with Cluster("Realtime Cache Cluster"):
    cache = Redis("cache-primary")
    cache - Redis("cache-replica") << metrics
    ingest_cluster >> cache
    api_cluster >> cache

  with Cluster("Messaging Cluster"):
    nats = Nats("nats-primary")
    nats - Nats("nats-secondary") << metrics
    ingest_cluster >> nats
    api_cluster << nats

  with Cluster("Database Cluster"):
    primary = PostgreSQL("primary")
    primary - PostgreSQL("replica1") << metrics
    primary - PostgreSQL("replica2") << metrics
    ingest_cluster >> primary
    api_cluster >> primary

  ingest_ingress >> ingest_cluster
  api_ingress >> api_cluster

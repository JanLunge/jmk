# The POG website on Nomad. Fabio routes pogv2.janlunge.de to it.
variable "VERSION" {
  description = "Tag of ghcr.io/janlunge/pogv2-site to run."
}
variable "ghcr_username" {}
variable "ghcr_token" {}

job "pogv2-site" {
  datacenters = ["in"]
  type        = "service"

  meta {
    version = var.VERSION
  }

  group "pogv2-site-group" {
    count = 1

    network {
      port "http" {
        to = 80
      }
    }

    update {
      max_parallel     = 1
      canary           = 1
      min_healthy_time = "10s"
      healthy_deadline = "3m"
      auto_promote     = true
      auto_revert      = true
    }

    task "pogv2-site-task" {
      driver = "docker"

      config {
        image = "ghcr.io/janlunge/pogv2-site:${var.VERSION}"
        ports = ["http"]
        auth {
          username = var.ghcr_username
          password = var.ghcr_token
        }
      }

      resources {
        cpu    = 50
        memory = 32
      }

      service {
        name = "pogv2-site"
        port = "http"
        tags = ["urlprefix-pogv2.janlunge.de/"]

        check {
          name     = "pogv2-site-http"
          type     = "http"
          path     = "/"
          interval = "10s"
          timeout  = "2s"
        }
      }
    }
  }
}

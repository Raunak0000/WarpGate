package com.warpgate.service;

import java.time.LocalDateTime;
import java.util.List;

import org.springframework.beans.factory.annotation.Value;
import org.springframework.http.HttpMethod;
import org.springframework.http.HttpStatus;
import org.springframework.stereotype.Service;
import org.springframework.web.client.HttpStatusCodeException;
import org.springframework.web.client.RestClient;
import org.springframework.web.client.RestClientException;

import com.fasterxml.jackson.databind.JsonNode;
import com.warpgate.dto.DeploymentRequest;
import com.warpgate.model.Deployment;
import com.warpgate.repository.DeploymentRepository;

@Service
public class DeploymentService {

    private final DeploymentRepository deploymentRepository;
    private final AuthService authService;
    private final RestClient orchestratorClient;

    public DeploymentService(
            DeploymentRepository deploymentRepository,
            AuthService authService,
            RestClient.Builder restClientBuilder,
            @Value("${warpgate.orchestrator.base-url}") String orchestratorUrl) {

        this.deploymentRepository = deploymentRepository;
        this.authService = authService;
        this.orchestratorClient =
                restClientBuilder.baseUrl(orchestratorUrl).build();
    }

    public JsonNode createDeployment(DeploymentRequest request) {
        return createDeployment(request, null);
    }

    public JsonNode createDeployment(DeploymentRequest request, String ownerUsername) {
        authService.deductQuota(ownerUsername, request.getTtlMinutes());

        LocalDateTime createdAt = LocalDateTime.now();
        LocalDateTime expiresAt =
                createdAt.plusMinutes(request.getTtlMinutes());

        Deployment deployment = new Deployment(
                request.getDeploymentId(),
                request.getRegion(),
                "PENDING",
                request.getTtlMinutes(),
                createdAt,
                expiresAt);
        deployment.setOwnerUsername(ownerUsername);

        deploymentRepository.save(deployment);

        try {
            JsonNode response = orchestratorClient.post()
                    .uri("/api/deployments")
                    .body(request)
                    .retrieve()
                    .body(JsonNode.class);

            if (response == null) {
                throw new IllegalStateException(
                        "Rust orchestrator returned an empty response");
            }

            return response;

        } catch (RestClientException exception) {
            authService.refundQuota(ownerUsername, request.getTtlMinutes());
            deployment.setStatus("FAILED");
            deploymentRepository.save(deployment);

            throw new org.springframework.web.server.ResponseStatusException(
                    HttpStatus.BAD_GATEWAY,
                    "Failed to submit deployment to the Rust orchestrator",
                    exception);
        }
    }

    public JsonNode getDeployment(String id) {
        try {
            JsonNode response = orchestratorClient.get()
                    .uri("/api/deployments/{id}", id)
                    .retrieve()
                    .body(JsonNode.class);

            if (response == null) {
                throw new IllegalStateException(
                        "Rust orchestrator returned an empty response");
            }

            if (response.has("status")) {
                String orchestratorStatus = response.get("status").asText();
                deploymentRepository.findByDeploymentId(id).ifPresent(dep -> {
                    if (!orchestratorStatus.equals(dep.getStatus())) {
                        dep.setStatus(orchestratorStatus);
                        deploymentRepository.save(dep);
                    }
                });
            }

            return response;

        } catch (RestClientException exception) {
            throw new org.springframework.web.server.ResponseStatusException(
                    HttpStatus.BAD_GATEWAY,
                    "Failed to retrieve deployment from the Rust orchestrator",
                    exception);
        }
    }

    public JsonNode teardownDeployment(String id, JsonNode request) {
        try {
            JsonNode response = orchestratorClient.post()
                    .uri("/api/deployments/{id}/teardown", id)
                    .body(request)
                    .retrieve()
                    .body(JsonNode.class);

            if (response == null) {
                throw new IllegalStateException(
                        "Rust orchestrator returned an empty response");
            }

            deploymentRepository.findByDeploymentId(id).ifPresent(dep -> {
                dep.setStatus("TERMINATING");
                deploymentRepository.save(dep);
            });

            return response;

        } catch (RestClientException exception) {
            throw new org.springframework.web.server.ResponseStatusException(
                    HttpStatus.BAD_GATEWAY,
                    "Failed to submit teardown request to the Rust orchestrator",
                    exception);
        }
    }

    public List<Deployment> getAllDeployments() {
        return deploymentRepository.findAll();
    }

    public void updateDeploymentFromWebhook(String id, JsonNode payload) {
        if (payload != null && payload.has("status")) {
            String status = payload.get("status").asText();
            deploymentRepository.findByDeploymentId(id).ifPresent(dep -> {
                dep.setStatus(status);
                deploymentRepository.save(dep);
            });
        }
    }

    public JsonNode addPeer(String id, JsonNode payload) {
        try {
            JsonNode response = orchestratorClient.post()
                    .uri("/api/deployments/{id}/peers", id)
                    .body(payload)
                    .retrieve()
                    .body(JsonNode.class);
            if (response == null) {
                throw new IllegalStateException("Rust orchestrator returned an empty response");
            }
            return response;
        } catch (HttpStatusCodeException exception) {
            throw new org.springframework.web.server.ResponseStatusException(
                    exception.getStatusCode(),
                    exception.getResponseBodyAsString(),
                    exception);
        } catch (RestClientException exception) {
            throw new org.springframework.web.server.ResponseStatusException(
                    HttpStatus.BAD_GATEWAY,
                    "Failed to add peer on the Rust orchestrator",
                    exception);
        }
    }

    public JsonNode listPeers(String id) {
        try {
            JsonNode response = orchestratorClient.get()
                    .uri("/api/deployments/{id}/peers", id)
                    .retrieve()
                    .body(JsonNode.class);
            if (response == null) {
                throw new IllegalStateException("Rust orchestrator returned an empty response");
            }
            return response;
        } catch (HttpStatusCodeException exception) {
            throw new org.springframework.web.server.ResponseStatusException(
                    exception.getStatusCode(),
                    exception.getResponseBodyAsString(),
                    exception);
        } catch (RestClientException exception) {
            throw new org.springframework.web.server.ResponseStatusException(
                    HttpStatus.BAD_GATEWAY,
                    "Failed to list peers from the Rust orchestrator",
                    exception);
        }
    }

    public JsonNode removePeer(String id, JsonNode payload) {
        try {
            JsonNode response = orchestratorClient.method(HttpMethod.DELETE)
                    .uri("/api/deployments/{id}/peers", id)
                    .body(payload)
                    .retrieve()
                    .body(JsonNode.class);
            if (response == null) {
                throw new IllegalStateException("Rust orchestrator returned an empty response");
            }
            return response;
        } catch (HttpStatusCodeException exception) {
            throw new org.springframework.web.server.ResponseStatusException(
                    exception.getStatusCode(),
                    exception.getResponseBodyAsString(),
                    exception);
        } catch (RestClientException exception) {
            throw new org.springframework.web.server.ResponseStatusException(
                    HttpStatus.BAD_GATEWAY,
                    "Failed to remove peer from the Rust orchestrator",
                    exception);
        }
    }
}
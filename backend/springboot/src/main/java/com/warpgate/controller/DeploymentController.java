package com.warpgate.controller;

import java.util.List;

import org.springframework.http.HttpStatus;
import org.springframework.http.ResponseEntity;
import org.springframework.web.bind.annotation.DeleteMapping;
import org.springframework.web.bind.annotation.GetMapping;
import org.springframework.web.bind.annotation.PathVariable;
import org.springframework.web.bind.annotation.PostMapping;
import org.springframework.web.bind.annotation.RequestBody;
import org.springframework.web.bind.annotation.RequestMapping;
import org.springframework.web.bind.annotation.RestController;

import com.fasterxml.jackson.databind.JsonNode;
import com.warpgate.config.JwtAuthInterceptor;
import com.warpgate.dto.DeploymentRequest;
import com.warpgate.model.Deployment;
import com.warpgate.service.DeploymentService;

import jakarta.servlet.http.HttpServletRequest;
import jakarta.validation.Valid;

@RestController
@RequestMapping("/api/deployments")
public class DeploymentController {

    private final DeploymentService deploymentService;

    public DeploymentController(DeploymentService deploymentService) {
        this.deploymentService = deploymentService;
    }

    @PostMapping
    public ResponseEntity<JsonNode> createDeployment(
            @Valid @RequestBody DeploymentRequest request,
            HttpServletRequest httpRequest) {

        Object attr = httpRequest.getAttribute(JwtAuthInterceptor.AUTHENTICATED_USER_ATTR);
        String username = attr instanceof String s ? s : null;
        JsonNode response = deploymentService.createDeployment(request, username);

        return ResponseEntity.status(HttpStatus.ACCEPTED).body(response);
    }

    @GetMapping
    public ResponseEntity<List<Deployment>> getDeployments() {
        return ResponseEntity.ok(deploymentService.getAllDeployments());
    }

    @GetMapping("/{id}")
    public ResponseEntity<JsonNode> getDeployment(
            @PathVariable String id) {

        return ResponseEntity.ok(
                deploymentService.getDeployment(id));
    }

    @PostMapping("/{id}/teardown")
    public ResponseEntity<JsonNode> teardownDeployment(
            @PathVariable String id,
            @RequestBody JsonNode payload) {

        return ResponseEntity.ok(
                deploymentService.teardownDeployment(id, payload));
    }

    @PostMapping("/{id}/webhook")
    public ResponseEntity<Void> receiveWebhook(
            @PathVariable String id,
            @RequestBody JsonNode payload) {

        deploymentService.updateDeploymentFromWebhook(id, payload);
        return ResponseEntity.ok().build();
    }

    @PostMapping("/{id}/peers")
    public ResponseEntity<JsonNode> addPeer(
            @PathVariable String id,
            @RequestBody JsonNode payload) {

        return ResponseEntity.status(HttpStatus.CREATED)
                .body(deploymentService.addPeer(id, payload));
    }

    @GetMapping("/{id}/peers")
    public ResponseEntity<JsonNode> listPeers(@PathVariable String id) {
        return ResponseEntity.ok(deploymentService.listPeers(id));
    }

    @DeleteMapping("/{id}/peers")
    public ResponseEntity<JsonNode> removePeer(
            @PathVariable String id,
            @RequestBody JsonNode payload) {

        return ResponseEntity.ok(deploymentService.removePeer(id, payload));
    }
}
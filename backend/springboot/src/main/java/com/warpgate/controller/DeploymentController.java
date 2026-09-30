package com.warpgate.controller;

import java.util.List;

import org.springframework.http.ResponseEntity;
import org.springframework.web.bind.annotation.GetMapping;
import org.springframework.web.bind.annotation.PathVariable;
import org.springframework.web.bind.annotation.PostMapping;
import org.springframework.web.bind.annotation.RequestBody;
import org.springframework.web.bind.annotation.RequestMapping;
import org.springframework.web.bind.annotation.RestController;

import com.warpgate.dto.DeploymentRequest;
import com.warpgate.model.Deployment;
import com.warpgate.service.DeploymentService;

import jakarta.validation.Valid;

@RestController
@RequestMapping("/api/deployments")
public class DeploymentController {

    private final DeploymentService deploymentService;

    public DeploymentController(DeploymentService deploymentService) {
        this.deploymentService = deploymentService;
    }

    @PostMapping
    public ResponseEntity<Deployment> createDeployment(
            @Valid @RequestBody DeploymentRequest request) {

        return ResponseEntity.ok(
                deploymentService.createDeployment(request));
    }

    @GetMapping
    public ResponseEntity<List<Deployment>> getDeployments() {
        return ResponseEntity.ok(
                deploymentService.getAllDeployments());
    }

    @GetMapping("/{id}")
    public ResponseEntity<Deployment> getDeployment(
            @PathVariable Long id) {

        return ResponseEntity.ok(
                deploymentService.getDeployment(id));
    }
}
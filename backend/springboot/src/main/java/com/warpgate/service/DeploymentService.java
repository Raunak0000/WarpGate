package com.warpgate.service;

import java.time.LocalDateTime;
import java.util.List;

import org.springframework.stereotype.Service;

import com.warpgate.dto.DeploymentRequest;
import com.warpgate.model.Deployment;
import com.warpgate.repository.DeploymentRepository;

@Service
public class DeploymentService {

    private final DeploymentRepository deploymentRepository;

    public DeploymentService(DeploymentRepository deploymentRepository) {
        this.deploymentRepository = deploymentRepository;
    }

    public Deployment createDeployment(DeploymentRequest request) {

        LocalDateTime createdAt = LocalDateTime.now();
        LocalDateTime expiresAt = createdAt.plusMinutes(request.getTtlMinutes());

        Deployment deployment = new Deployment(
                request.getRegion(),
                "PENDING",
                request.getTtlMinutes(),
                createdAt,
                expiresAt);

        return deploymentRepository.save(deployment);
    }

    public List<Deployment> getAllDeployments() {
        return deploymentRepository.findAll();
    }

    public Deployment getDeployment(Long id) {
        return deploymentRepository.findById(id)
                .orElseThrow(() -> new RuntimeException("Deployment not found: " + id));
    }
}
package com.warpgate.repository;

import java.util.Optional;

import org.springframework.data.jpa.repository.JpaRepository;
import com.warpgate.model.Deployment;

public interface DeploymentRepository extends JpaRepository<Deployment, Long> {
    Optional<Deployment> findByDeploymentId(String deploymentId);
}
package com.warpgate.dto;

import com.fasterxml.jackson.annotation.JsonProperty;

import jakarta.validation.constraints.Min;
import jakarta.validation.constraints.NotBlank;
import jakarta.validation.constraints.NotNull;

public class DeploymentRequest {

    @NotBlank
    @JsonProperty("deployment_id")
    private String deploymentId;

    @NotBlank
    private String region;

    @NotNull
    @Min(1)
    @JsonProperty("ttl_minutes")
    private Integer ttlMinutes;

    @NotBlank
    @JsonProperty("client_public_key")
    private String clientPublicKey;

    @NotBlank
    @JsonProperty("admin_cidr")
    private String adminCidr;

    public String getDeploymentId() {
        return deploymentId;
    }

    public void setDeploymentId(String deploymentId) {
        this.deploymentId = deploymentId;
    }

    public String getRegion() {
        return region;
    }

    public void setRegion(String region) {
        this.region = region;
    }

    public Integer getTtlMinutes() {
        return ttlMinutes;
    }

    public void setTtlMinutes(Integer ttlMinutes) {
        this.ttlMinutes = ttlMinutes;
    }

    public String getClientPublicKey() {
        return clientPublicKey;
    }

    public void setClientPublicKey(String clientPublicKey) {
        this.clientPublicKey = clientPublicKey;
    }

    public String getAdminCidr() {
        return adminCidr;
    }

    public void setAdminCidr(String adminCidr) {
        this.adminCidr = adminCidr;
    }
}
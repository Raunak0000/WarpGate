package com.warpgate.controller;

import java.time.LocalDateTime;
import java.util.List;

import org.junit.jupiter.api.Test;
import org.mockito.Mockito;
import org.springframework.beans.factory.annotation.Autowired;
import org.springframework.boot.test.autoconfigure.web.servlet.WebMvcTest;
import org.springframework.boot.test.mock.mockito.MockBean;
import org.springframework.http.HttpStatus;
import org.springframework.http.MediaType;
import org.springframework.test.web.servlet.MockMvc;
import org.springframework.web.server.ResponseStatusException;

import com.fasterxml.jackson.databind.ObjectMapper;
import com.fasterxml.jackson.databind.node.ObjectNode;
import com.warpgate.dto.DeploymentRequest;
import com.warpgate.model.Deployment;
import com.warpgate.service.DeploymentService;

import static org.mockito.ArgumentMatchers.any;
import static org.mockito.ArgumentMatchers.eq;
import static org.springframework.test.web.servlet.request.MockMvcRequestBuilders.get;
import static org.springframework.test.web.servlet.request.MockMvcRequestBuilders.post;
import static org.springframework.test.web.servlet.result.MockMvcResultMatchers.jsonPath;
import static org.springframework.test.web.servlet.result.MockMvcResultMatchers.status;

@WebMvcTest(DeploymentController.class)
public class DeploymentControllerTest {

    @Autowired
    private MockMvc mockMvc;

    @Autowired
    private ObjectMapper objectMapper;

    @MockBean
    private DeploymentService deploymentService;

    @Test
    void testCreateDeploymentSuccess() throws Exception {
        DeploymentRequest request = new DeploymentRequest();
        request.setDeploymentId("warp-test-1");
        request.setRegion("us-east-1");
        request.setTtlMinutes(30);
        request.setClientPublicKey("dummyPublicKey=");
        request.setAdminCidr("0.0.0.0/0");

        ObjectNode responseNode = objectMapper.createObjectNode();
        responseNode.put("deployment_id", "warp-test-1");
        responseNode.put("status", "PROVISIONING_INFRA");

        Mockito.when(deploymentService.createDeployment(any(DeploymentRequest.class)))
                .thenReturn(responseNode);

        mockMvc.perform(post("/api/deployments")
                .contentType(MediaType.APPLICATION_JSON)
                .content(objectMapper.writeValueAsString(request)))
                .andExpect(status().isAccepted())
                .andExpect(jsonPath("$.deployment_id").value("warp-test-1"))
                .andExpect(jsonPath("$.status").value("PROVISIONING_INFRA"));
    }

    @Test
    void testCreateDeploymentValidationError() throws Exception {
        DeploymentRequest invalidRequest = new DeploymentRequest();
        // Missing required fields

        mockMvc.perform(post("/api/deployments")
                .contentType(MediaType.APPLICATION_JSON)
                .content(objectMapper.writeValueAsString(invalidRequest)))
                .andExpect(status().isBadRequest());
    }

    @Test
    void testGetDeploymentSuccess() throws Exception {
        ObjectNode responseNode = objectMapper.createObjectNode();
        responseNode.put("deployment_id", "warp-test-1");
        responseNode.put("status", "READY");

        Mockito.when(deploymentService.getDeployment("warp-test-1"))
                .thenReturn(responseNode);

        mockMvc.perform(get("/api/deployments/warp-test-1"))
                .andExpect(status().isOk())
                .andExpect(jsonPath("$.deployment_id").value("warp-test-1"))
                .andExpect(jsonPath("$.status").value("READY"));
    }

    @Test
    void testGetDeploymentOrchestratorFailure() throws Exception {
        Mockito.when(deploymentService.getDeployment("nonexistent"))
                .thenThrow(new ResponseStatusException(HttpStatus.BAD_GATEWAY, "Failed"));

        mockMvc.perform(get("/api/deployments/nonexistent"))
                .andExpect(status().isBadGateway());
    }

    @Test
    void testGetAllDeployments() throws Exception {
        Deployment d1 = new Deployment("warp-1", "us-east-1", "READY", 30,
                LocalDateTime.now(), LocalDateTime.now().plusMinutes(30));
        Mockito.when(deploymentService.getAllDeployments())
                .thenReturn(List.of(d1));

        mockMvc.perform(get("/api/deployments"))
                .andExpect(status().isOk())
                .andExpect(jsonPath("$[0].deploymentId").value("warp-1"))
                .andExpect(jsonPath("$[0].region").value("us-east-1"))
                .andExpect(jsonPath("$[0].status").value("READY"));
    }

    @Test
    void testTeardownDeploymentSuccess() throws Exception {
        ObjectNode responseNode = objectMapper.createObjectNode();
        responseNode.put("deployment_id", "warp-test-1");
        responseNode.put("status", "TERMINATING");
        responseNode.put("message", "Teardown initiated");

        ObjectNode payload = objectMapper.createObjectNode();
        payload.put("deployment_id", "warp-test-1");
        payload.put("region", "us-east-1");

        Mockito.when(deploymentService.teardownDeployment(eq("warp-test-1"), any()))
                .thenReturn(responseNode);

        mockMvc.perform(post("/api/deployments/warp-test-1/teardown")
                .contentType(MediaType.APPLICATION_JSON)
                .content(objectMapper.writeValueAsString(payload)))
                .andExpect(status().isOk())
                .andExpect(jsonPath("$.status").value("TERMINATING"))
                .andExpect(jsonPath("$.message").value("Teardown initiated"));
    }
}

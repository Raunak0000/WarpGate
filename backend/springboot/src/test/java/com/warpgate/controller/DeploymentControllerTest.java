package com.warpgate.controller;

import java.time.LocalDateTime;
import java.util.List;
import java.util.Optional;

import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.Test;
import org.mockito.Mockito;
import org.springframework.beans.factory.annotation.Autowired;
import org.springframework.boot.test.autoconfigure.web.servlet.WebMvcTest;
import org.springframework.boot.test.mock.mockito.MockBean;
import org.springframework.http.HttpHeaders;
import org.springframework.http.HttpStatus;
import org.springframework.http.MediaType;
import org.springframework.test.web.servlet.MockMvc;
import org.springframework.web.server.ResponseStatusException;

import com.fasterxml.jackson.databind.ObjectMapper;
import com.fasterxml.jackson.databind.node.ObjectNode;
import com.warpgate.dto.DeploymentRequest;
import com.warpgate.model.Deployment;
import com.warpgate.service.DeploymentService;
import com.warpgate.service.JwtService;

import static org.mockito.ArgumentMatchers.any;
import static org.mockito.ArgumentMatchers.eq;
import static org.springframework.test.web.servlet.request.MockMvcRequestBuilders.delete;
import static org.springframework.test.web.servlet.request.MockMvcRequestBuilders.get;
import static org.springframework.test.web.servlet.request.MockMvcRequestBuilders.post;
import static org.springframework.test.web.servlet.result.MockMvcResultMatchers.jsonPath;
import static org.springframework.test.web.servlet.result.MockMvcResultMatchers.status;

@WebMvcTest(DeploymentController.class)
public class DeploymentControllerTest {

    private static final String TEST_AUTH_HEADER = "Bearer test-jwt-token";

    @Autowired
    private MockMvc mockMvc;

    @Autowired
    private ObjectMapper objectMapper;

    @MockBean
    private DeploymentService deploymentService;

    @MockBean
    private JwtService jwtService;

    @BeforeEach
    void setupAuth() {
        Mockito.when(jwtService.validateAndExtractUsername("test-jwt-token"))
                .thenReturn(Optional.of("alice"));
    }

    @Test
    void testCreateDeploymentRequiresAuth() throws Exception {
        DeploymentRequest request = new DeploymentRequest();
        request.setDeploymentId("warp-unauth");
        request.setRegion("us-east-1");
        request.setTtlMinutes(30);
        request.setClientPublicKey("dummyPublicKey=");
        request.setAdminCidr("0.0.0.0/0");

        mockMvc.perform(post("/api/deployments")
                .contentType(MediaType.APPLICATION_JSON)
                .content(objectMapper.writeValueAsString(request)))
                .andExpect(status().isUnauthorized());
    }

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

        Mockito.when(deploymentService.createDeployment(any(DeploymentRequest.class), eq("alice")))
                .thenReturn(responseNode);

        mockMvc.perform(post("/api/deployments")
                .header(HttpHeaders.AUTHORIZATION, TEST_AUTH_HEADER)
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
                .header(HttpHeaders.AUTHORIZATION, TEST_AUTH_HEADER)
                .contentType(MediaType.APPLICATION_JSON)
                .content(objectMapper.writeValueAsString(invalidRequest)))
                .andExpect(status().isBadRequest());
    }

    @Test
    void testCreateDeploymentExceedsMaxTtl() throws Exception {
        DeploymentRequest exceedRequest = new DeploymentRequest();
        exceedRequest.setDeploymentId("warp-exceed");
        exceedRequest.setRegion("us-east-1");
        exceedRequest.setTtlMinutes(121); // Exceeds 120m max
        exceedRequest.setClientPublicKey("dummyPublicKey=");
        exceedRequest.setAdminCidr("0.0.0.0/0");

        mockMvc.perform(post("/api/deployments")
                .header(HttpHeaders.AUTHORIZATION, TEST_AUTH_HEADER)
                .contentType(MediaType.APPLICATION_JSON)
                .content(objectMapper.writeValueAsString(exceedRequest)))
                .andExpect(status().isBadRequest());
    }

    @Test
    void testReceiveWebhookSuccessWithoutAuth() throws Exception {
        ObjectNode webhookPayload = objectMapper.createObjectNode();
        webhookPayload.put("status", "READY");

        mockMvc.perform(post("/api/deployments/warp-test-1/webhook")
                .contentType(MediaType.APPLICATION_JSON)
                .content(objectMapper.writeValueAsString(webhookPayload)))
                .andExpect(status().isOk());

        Mockito.verify(deploymentService).updateDeploymentFromWebhook(eq("warp-test-1"), any());
    }

    @Test
    void testGetDeploymentSuccess() throws Exception {
        ObjectNode responseNode = objectMapper.createObjectNode();
        responseNode.put("deployment_id", "warp-test-1");
        responseNode.put("status", "READY");

        Mockito.when(deploymentService.getDeployment("warp-test-1"))
                .thenReturn(responseNode);

        mockMvc.perform(get("/api/deployments/warp-test-1")
                .header(HttpHeaders.AUTHORIZATION, TEST_AUTH_HEADER))
                .andExpect(status().isOk())
                .andExpect(jsonPath("$.deployment_id").value("warp-test-1"))
                .andExpect(jsonPath("$.status").value("READY"));
    }

    @Test
    void testGetDeploymentOrchestratorFailure() throws Exception {
        Mockito.when(deploymentService.getDeployment("nonexistent"))
                .thenThrow(new ResponseStatusException(HttpStatus.BAD_GATEWAY, "Failed"));

        mockMvc.perform(get("/api/deployments/nonexistent")
                .header(HttpHeaders.AUTHORIZATION, TEST_AUTH_HEADER))
                .andExpect(status().isBadGateway());
    }

    @Test
    void testGetAllDeployments() throws Exception {
        Deployment d1 = new Deployment("warp-1", "us-east-1", "READY", 30,
                LocalDateTime.now(), LocalDateTime.now().plusMinutes(30));
        Mockito.when(deploymentService.getAllDeployments())
                .thenReturn(List.of(d1));

        mockMvc.perform(get("/api/deployments")
                .header(HttpHeaders.AUTHORIZATION, TEST_AUTH_HEADER))
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
                .header(HttpHeaders.AUTHORIZATION, TEST_AUTH_HEADER)
                .contentType(MediaType.APPLICATION_JSON)
                .content(objectMapper.writeValueAsString(payload)))
                .andExpect(status().isOk())
                .andExpect(jsonPath("$.status").value("TERMINATING"))
                .andExpect(jsonPath("$.message").value("Teardown initiated"));
    }

    @Test
    void testAddListAndRemovePeers() throws Exception {
        ObjectNode peerResponse = objectMapper.createObjectNode();
        peerResponse.put("deployment_id", "warp-test-1");
        peerResponse.put("device_name", "pixel-phone");
        peerResponse.put("assigned_ip", "10.200.0.3/32");

        Mockito.when(deploymentService.addPeer(eq("warp-test-1"), any()))
                .thenReturn(peerResponse);

        ObjectNode addPayload = objectMapper.createObjectNode();
        addPayload.put("device_name", "pixel-phone");
        addPayload.put("public_key", "M0pTVJqVNONA94wmPZCeNGDnEKSDXvu+yXEqtvqpp1I=");

        mockMvc.perform(post("/api/deployments/warp-test-1/peers")
                .header(HttpHeaders.AUTHORIZATION, TEST_AUTH_HEADER)
                .contentType(MediaType.APPLICATION_JSON)
                .content(objectMapper.writeValueAsString(addPayload)))
                .andExpect(status().isCreated())
                .andExpect(jsonPath("$.assigned_ip").value("10.200.0.3/32"))
                .andExpect(jsonPath("$.device_name").value("pixel-phone"));

        Mockito.when(deploymentService.listPeers("warp-test-1"))
                .thenReturn(objectMapper.createArrayNode().add(peerResponse));

        mockMvc.perform(get("/api/deployments/warp-test-1/peers")
                .header(HttpHeaders.AUTHORIZATION, TEST_AUTH_HEADER))
                .andExpect(status().isOk())
                .andExpect(jsonPath("$[0].device_name").value("pixel-phone"));

        ObjectNode removeResponse = objectMapper.createObjectNode();
        removeResponse.put("status", "REMOVED");
        Mockito.when(deploymentService.removePeer(eq("warp-test-1"), any()))
                .thenReturn(removeResponse);

        ObjectNode delPayload = objectMapper.createObjectNode();
        delPayload.put("public_key", "M0pTVJqVNONA94wmPZCeNGDnEKSDXvu+yXEqtvqpp1I=");

        mockMvc.perform(delete("/api/deployments/warp-test-1/peers")
                .header(HttpHeaders.AUTHORIZATION, TEST_AUTH_HEADER)
                .contentType(MediaType.APPLICATION_JSON)
                .content(objectMapper.writeValueAsString(delPayload)))
                .andExpect(status().isOk())
                .andExpect(jsonPath("$.status").value("REMOVED"));
    }
}

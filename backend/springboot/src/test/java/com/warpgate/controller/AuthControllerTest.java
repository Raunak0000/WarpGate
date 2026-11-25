package com.warpgate.controller;

import com.fasterxml.jackson.databind.ObjectMapper;
import com.warpgate.dto.AuthResponse;
import com.warpgate.dto.LoginRequest;
import com.warpgate.dto.RegisterRequest;
import com.warpgate.dto.UserProfileResponse;
import com.warpgate.service.AuthService;
import com.warpgate.service.JwtService;
import java.time.LocalDateTime;
import java.util.Optional;
import org.junit.jupiter.api.Test;
import org.mockito.Mockito;
import org.springframework.beans.factory.annotation.Autowired;
import org.springframework.boot.test.autoconfigure.web.servlet.WebMvcTest;
import org.springframework.boot.test.mock.mockito.MockBean;
import org.springframework.http.HttpHeaders;
import org.springframework.http.MediaType;
import org.springframework.test.web.servlet.MockMvc;

import static org.mockito.ArgumentMatchers.any;
import static org.springframework.test.web.servlet.request.MockMvcRequestBuilders.get;
import static org.springframework.test.web.servlet.request.MockMvcRequestBuilders.post;
import static org.springframework.test.web.servlet.result.MockMvcResultMatchers.jsonPath;
import static org.springframework.test.web.servlet.result.MockMvcResultMatchers.status;

@WebMvcTest(AuthController.class)
public class AuthControllerTest {

    @Autowired
    private MockMvc mockMvc;

    @Autowired
    private ObjectMapper objectMapper;

    @MockBean
    private AuthService authService;

    @MockBean
    private JwtService jwtService;

    @Test
    void testRegisterSuccess() throws Exception {
        RegisterRequest req = new RegisterRequest();
        req.setUsername("alice");
        req.setEmail("alice@example.com");
        req.setPassword("secretpass123");

        AuthResponse resp = new AuthResponse("jwt-token-123", "alice", "alice@example.com", 600, 600);
        Mockito.when(authService.register(any(RegisterRequest.class))).thenReturn(resp);

        mockMvc.perform(post("/api/auth/register")
                .contentType(MediaType.APPLICATION_JSON)
                .content(objectMapper.writeValueAsString(req)))
                .andExpect(status().isCreated())
                .andExpect(jsonPath("$.token").value("jwt-token-123"))
                .andExpect(jsonPath("$.username").value("alice"))
                .andExpect(jsonPath("$.monthly_quota_minutes").value(600))
                .andExpect(jsonPath("$.remaining_quota_minutes").value(600));
    }

    @Test
    void testLoginSuccess() throws Exception {
        LoginRequest req = new LoginRequest();
        req.setUsername("alice");
        req.setPassword("secretpass123");

        AuthResponse resp = new AuthResponse("jwt-token-456", "alice", "alice@example.com", 600, 570);
        Mockito.when(authService.login(any(LoginRequest.class))).thenReturn(resp);

        mockMvc.perform(post("/api/auth/login")
                .contentType(MediaType.APPLICATION_JSON)
                .content(objectMapper.writeValueAsString(req)))
                .andExpect(status().isOk())
                .andExpect(jsonPath("$.token").value("jwt-token-456"))
                .andExpect(jsonPath("$.remaining_quota_minutes").value(570));
    }

    @Test
    void testMeRequiresAuth() throws Exception {
        mockMvc.perform(get("/api/auth/me"))
                .andExpect(status().isUnauthorized());
    }

    @Test
    void testMeSuccess() throws Exception {
        Mockito.when(jwtService.validateAndExtractUsername("valid-token"))
                .thenReturn(Optional.of("alice"));
        Mockito.when(authService.getProfile("alice"))
                .thenReturn(new UserProfileResponse("alice", "alice@example.com", 600, 540, LocalDateTime.now().plusDays(20)));

        mockMvc.perform(get("/api/auth/me")
                .header(HttpHeaders.AUTHORIZATION, "Bearer valid-token"))
                .andExpect(status().isOk())
                .andExpect(jsonPath("$.username").value("alice"))
                .andExpect(jsonPath("$.remaining_quota_minutes").value(540));
    }
}


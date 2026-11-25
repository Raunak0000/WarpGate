package com.warpgate.service;

import com.fasterxml.jackson.databind.ObjectMapper;
import org.junit.jupiter.api.Test;

import static org.junit.jupiter.api.Assertions.*;

public class JwtAndPasswordTest {

    @Test
    void testPasswordHashAndVerify() {
        PasswordService passwordService = new PasswordService();
        String hash = passwordService.hashPassword("superSecret99!");

        assertNotNull(hash);
        assertTrue(hash.contains(":"));
        assertTrue(passwordService.verifyPassword("superSecret99!", hash));
        assertFalse(passwordService.verifyPassword("wrongPassword", hash));
    }

    @Test
    void testJwtGenerateAndValidate() {
        JwtService jwtService = new JwtService("test-secret-key-at-least-32-bytes-long", 3600, new ObjectMapper());
        String token = jwtService.generateToken("alice");

        assertNotNull(token);
        assertEquals(3, token.split("\\.").length);
        assertEquals("alice", jwtService.validateAndExtractUsername(token).orElseThrow());

        // Tampered token must fail validation
        String tampered = token.substring(0, token.length() - 2) + "xx";
        assertTrue(jwtService.validateAndExtractUsername(tampered).isEmpty());
    }
}


package com.warpgate.service;

import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.ObjectMapper;
import com.fasterxml.jackson.databind.node.ObjectNode;
import java.nio.charset.StandardCharsets;
import java.security.GeneralSecurityException;
import java.security.MessageDigest;
import java.time.Instant;
import java.util.Base64;
import java.util.Optional;
import javax.crypto.Mac;
import javax.crypto.spec.SecretKeySpec;
import org.springframework.beans.factory.annotation.Value;
import org.springframework.stereotype.Service;

/**
 * Embedded HS256 JWT token issuer and validator with zero external dependencies.
 */
@Service
public class JwtService {

    private static final String HMAC_ALG = "HmacSHA256";
    private static final Base64.Encoder URL_ENCODER = Base64.getUrlEncoder().withoutPadding();
    private static final Base64.Decoder URL_DECODER = Base64.getUrlDecoder();

    private final byte[] secretBytes;
    private final long expirationSeconds;
    private final ObjectMapper objectMapper;

    public JwtService(
            @Value("${warpgate.auth.jwt-secret:warpgate-dev-secret-change-in-production-32bytes}") String secret,
            @Value("${warpgate.auth.jwt-expiration-seconds:86400}") long expirationSeconds,
            ObjectMapper objectMapper) {
        this.secretBytes = secret.getBytes(StandardCharsets.UTF_8);
        this.expirationSeconds = expirationSeconds;
        this.objectMapper = objectMapper;
    }

    public String generateToken(String username) {
        try {
            long now = Instant.now().getEpochSecond();
            long exp = now + expirationSeconds;

            ObjectNode header = objectMapper.createObjectNode();
            header.put("alg", "HS256");
            header.put("typ", "JWT");

            ObjectNode payload = objectMapper.createObjectNode();
            payload.put("sub", username);
            payload.put("iat", now);
            payload.put("exp", exp);

            String encodedHeader = URL_ENCODER.encodeToString(objectMapper.writeValueAsBytes(header));
            String encodedPayload = URL_ENCODER.encodeToString(objectMapper.writeValueAsBytes(payload));
            String signingInput = encodedHeader + "." + encodedPayload;
            String signature = sign(signingInput);

            return signingInput + "." + signature;
        } catch (Exception e) {
            throw new IllegalStateException("Failed to generate JWT token", e);
        }
    }

    public Optional<String> validateAndExtractUsername(String token) {
        if (token == null || token.isBlank()) {
            return Optional.empty();
        }
        String[] parts = token.split("\\.");
        if (parts.length != 3) {
            return Optional.empty();
        }

        String signingInput = parts[0] + "." + parts[1];
        String expectedSignature = sign(signingInput);
        if (!MessageDigest.isEqual(
                expectedSignature.getBytes(StandardCharsets.UTF_8),
                parts[2].getBytes(StandardCharsets.UTF_8))) {
            return Optional.empty();
        }

        try {
            byte[] payloadBytes = URL_DECODER.decode(parts[1]);
            JsonNode payload = objectMapper.readTree(payloadBytes);
            if (!payload.has("sub") || !payload.has("exp")) {
                return Optional.empty();
            }
            long exp = payload.get("exp").asLong();
            if (Instant.now().getEpochSecond() >= exp) {
                return Optional.empty();
            }
            String sub = payload.get("sub").asText();
            return sub.isBlank() ? Optional.empty() : Optional.of(sub);
        } catch (Exception e) {
            return Optional.empty();
        }
    }

    private String sign(String data) {
        try {
            Mac mac = Mac.getInstance(HMAC_ALG);
            mac.init(new SecretKeySpec(secretBytes, HMAC_ALG));
            byte[] rawSig = mac.doFinal(data.getBytes(StandardCharsets.UTF_8));
            return URL_ENCODER.encodeToString(rawSig);
        } catch (GeneralSecurityException e) {
            throw new IllegalStateException("Failed to sign JWT", e);
        }
    }
}


package com.warpgate.config;

import com.warpgate.service.JwtService;
import jakarta.servlet.http.HttpServletRequest;
import jakarta.servlet.http.HttpServletResponse;
import java.io.IOException;
import java.util.Optional;
import org.springframework.http.HttpHeaders;
import org.springframework.http.MediaType;
import org.springframework.stereotype.Component;
import org.springframework.web.servlet.HandlerInterceptor;

/**
 * Validates Bearer JWT tokens on protected API routes.
 */
@Component
public class JwtAuthInterceptor implements HandlerInterceptor {

    public static final String AUTHENTICATED_USER_ATTR = "authenticatedUser";

    private final JwtService jwtService;

    public JwtAuthInterceptor(JwtService jwtService) {
        this.jwtService = jwtService;
    }

    @Override
    public boolean preHandle(HttpServletRequest request, HttpServletResponse response, Object handler)
            throws IOException {
        String path = request.getRequestURI();

        // Allow internal orchestrator lifecycle webhooks without user JWT
        if (path.matches("^/api/deployments/[^/]+/webhook$")) {
            return true;
        }

        String authHeader = request.getHeader(HttpHeaders.AUTHORIZATION);
        if (authHeader == null || !authHeader.startsWith("Bearer ")) {
            sendUnauthorized(response, "Missing or invalid Authorization header. Run 'warpgate login' first.");
            return false;
        }

        String token = authHeader.substring("Bearer ".length()).trim();
        Optional<String> username = jwtService.validateAndExtractUsername(token);
        if (username.isEmpty()) {
            sendUnauthorized(response, "Invalid or expired JWT token. Run 'warpgate login' to refresh.");
            return false;
        }

        request.setAttribute(AUTHENTICATED_USER_ATTR, username.get());
        return true;
    }

    private void sendUnauthorized(HttpServletResponse response, String message) throws IOException {
        response.setStatus(HttpServletResponse.SC_UNAUTHORIZED);
        response.setContentType(MediaType.APPLICATION_JSON_VALUE);
        response.getWriter().write("{\"error\":\"" + message.replace("\"", "\\\"") + "\"}");
    }
}


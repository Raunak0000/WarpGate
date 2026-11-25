package com.warpgate.service;

import com.warpgate.dto.AuthResponse;
import com.warpgate.dto.LoginRequest;
import com.warpgate.dto.RegisterRequest;
import com.warpgate.dto.UserProfileResponse;
import com.warpgate.model.User;
import com.warpgate.repository.UserRepository;
import java.time.LocalDateTime;
import java.util.Optional;
import org.springframework.http.HttpStatus;
import org.springframework.stereotype.Service;
import org.springframework.transaction.annotation.Transactional;
import org.springframework.web.server.ResponseStatusException;

@Service
public class AuthService {

    private final UserRepository userRepository;
    private final PasswordService passwordService;
    private final JwtService jwtService;

    public AuthService(
            UserRepository userRepository,
            PasswordService passwordService,
            JwtService jwtService) {
        this.userRepository = userRepository;
        this.passwordService = passwordService;
        this.jwtService = jwtService;
    }

    @Transactional
    public AuthResponse register(RegisterRequest request) {
        String username = request.getUsername().trim();
        String email = request.getEmail().trim().toLowerCase();

        if (userRepository.existsByUsername(username)) {
            throw new ResponseStatusException(HttpStatus.CONFLICT, "Username already exists");
        }
        if (userRepository.existsByEmail(email)) {
            throw new ResponseStatusException(HttpStatus.CONFLICT, "Email already registered");
        }

        LocalDateTime now = LocalDateTime.now();
        LocalDateTime nextReset = now.plusMonths(1);
        String passwordHash = passwordService.hashPassword(request.getPassword());

        User user = new User(username, email, passwordHash, now, nextReset);
        userRepository.save(user);

        String token = jwtService.generateToken(user.getUsername());
        return new AuthResponse(
                token,
                user.getUsername(),
                user.getEmail(),
                user.getMonthlyQuotaMinutes(),
                user.getRemainingQuotaMinutes());
    }

    @Transactional
    public AuthResponse login(LoginRequest request) {
        String identifier = request.getUsername().trim();
        Optional<User> userOpt = userRepository.findByUsername(identifier);
        if (userOpt.isEmpty()) {
            userOpt = userRepository.findByEmail(identifier.toLowerCase());
        }

        User user = userOpt.orElseThrow(() ->
                new ResponseStatusException(HttpStatus.UNAUTHORIZED, "Invalid username or password"));

        if (!passwordService.verifyPassword(request.getPassword(), user.getPasswordHash())) {
            throw new ResponseStatusException(HttpStatus.UNAUTHORIZED, "Invalid username or password");
        }

        refreshQuotaIfExpired(user);
        String token = jwtService.generateToken(user.getUsername());

        return new AuthResponse(
                token,
                user.getUsername(),
                user.getEmail(),
                user.getMonthlyQuotaMinutes(),
                user.getRemainingQuotaMinutes());
    }

    @Transactional
    public UserProfileResponse getProfile(String username) {
        User user = userRepository.findByUsername(username)
                .orElseThrow(() -> new ResponseStatusException(HttpStatus.NOT_FOUND, "User not found"));
        refreshQuotaIfExpired(user);
        return new UserProfileResponse(
                user.getUsername(),
                user.getEmail(),
                user.getMonthlyQuotaMinutes(),
                user.getRemainingQuotaMinutes(),
                user.getQuotaResetAt());
    }

    @Transactional
    public void deductQuota(String username, int minutes) {
        if (username == null || username.isBlank()) {
            return;
        }
        User user = userRepository.findByUsername(username)
                .orElseThrow(() -> new ResponseStatusException(HttpStatus.UNAUTHORIZED, "Authenticated user not found"));

        refreshQuotaIfExpired(user);

        if (user.getRemainingQuotaMinutes() < minutes) {
            throw new ResponseStatusException(
                    HttpStatus.PAYMENT_REQUIRED,
                    String.format(
                            "Monthly virtual quota exceeded: requested %d minutes, but only %d/%d minutes remaining (resets %s)",
                            minutes,
                            user.getRemainingQuotaMinutes(),
                            user.getMonthlyQuotaMinutes(),
                            user.getQuotaResetAt()));
        }

        user.setRemainingQuotaMinutes(user.getRemainingQuotaMinutes() - minutes);
        userRepository.save(user);
    }

    @Transactional
    public void refundQuota(String username, int minutes) {
        if (username == null || username.isBlank()) {
            return;
        }
        userRepository.findByUsername(username).ifPresent(user -> {
            int restored = Math.min(user.getMonthlyQuotaMinutes(), user.getRemainingQuotaMinutes() + minutes);
            user.setRemainingQuotaMinutes(restored);
            userRepository.save(user);
        });
    }

    private void refreshQuotaIfExpired(User user) {
        LocalDateTime now = LocalDateTime.now();
        if (user.getQuotaResetAt() != null && now.isAfter(user.getQuotaResetAt())) {
            user.setRemainingQuotaMinutes(user.getMonthlyQuotaMinutes());
            user.setQuotaResetAt(now.plusMonths(1));
            userRepository.save(user);
        }
    }
}


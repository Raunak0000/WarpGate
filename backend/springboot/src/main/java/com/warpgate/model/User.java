package com.warpgate.model;

import jakarta.persistence.*;
import java.time.LocalDateTime;

@Entity
@Table(name = "users")
public class User {

    public static final int DEFAULT_MONTHLY_QUOTA_MINUTES = 600;

    @Id
    @GeneratedValue(strategy = GenerationType.IDENTITY)
    private Long id;

    @Column(nullable = false, unique = true)
    private String username;

    @Column(nullable = false, unique = true)
    private String email;

    @Column(nullable = false)
    private String passwordHash;

    @Column(nullable = false)
    private Integer monthlyQuotaMinutes = DEFAULT_MONTHLY_QUOTA_MINUTES;

    @Column(nullable = false)
    private Integer remainingQuotaMinutes = DEFAULT_MONTHLY_QUOTA_MINUTES;

    @Column(nullable = false)
    private LocalDateTime quotaResetAt;

    @Column(nullable = false)
    private LocalDateTime createdAt;

    public User() {
    }

    public User(String username, String email, String passwordHash, LocalDateTime createdAt, LocalDateTime quotaResetAt) {
        this.username = username;
        this.email = email;
        this.passwordHash = passwordHash;
        this.monthlyQuotaMinutes = DEFAULT_MONTHLY_QUOTA_MINUTES;
        this.remainingQuotaMinutes = DEFAULT_MONTHLY_QUOTA_MINUTES;
        this.createdAt = createdAt;
        this.quotaResetAt = quotaResetAt;
    }

    public Long getId() {
        return id;
    }

    public String getUsername() {
        return username;
    }

    public void setUsername(String username) {
        this.username = username;
    }

    public String getEmail() {
        return email;
    }

    public void setEmail(String email) {
        this.email = email;
    }

    public String getPasswordHash() {
        return passwordHash;
    }

    public void setPasswordHash(String passwordHash) {
        this.passwordHash = passwordHash;
    }

    public Integer getMonthlyQuotaMinutes() {
        return monthlyQuotaMinutes;
    }

    public void setMonthlyQuotaMinutes(Integer monthlyQuotaMinutes) {
        this.monthlyQuotaMinutes = monthlyQuotaMinutes;
    }

    public Integer getRemainingQuotaMinutes() {
        return remainingQuotaMinutes;
    }

    public void setRemainingQuotaMinutes(Integer remainingQuotaMinutes) {
        this.remainingQuotaMinutes = remainingQuotaMinutes;
    }

    public LocalDateTime getQuotaResetAt() {
        return quotaResetAt;
    }

    public void setQuotaResetAt(LocalDateTime quotaResetAt) {
        this.quotaResetAt = quotaResetAt;
    }

    public LocalDateTime getCreatedAt() {
        return createdAt;
    }

    public void setCreatedAt(LocalDateTime createdAt) {
        this.createdAt = createdAt;
    }
}


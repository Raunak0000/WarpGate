package com.warpgate.dto;

import com.fasterxml.jackson.annotation.JsonProperty;

public class AuthResponse {

    private String token;
    private String username;
    private String email;

    @JsonProperty("monthly_quota_minutes")
    private Integer monthlyQuotaMinutes;

    @JsonProperty("remaining_quota_minutes")
    private Integer remainingQuotaMinutes;

    public AuthResponse() {
    }

    public AuthResponse(String token, String username, String email,
            Integer monthlyQuotaMinutes, Integer remainingQuotaMinutes) {
        this.token = token;
        this.username = username;
        this.email = email;
        this.monthlyQuotaMinutes = monthlyQuotaMinutes;
        this.remainingQuotaMinutes = remainingQuotaMinutes;
    }

    public String getToken() {
        return token;
    }

    public String getUsername() {
        return username;
    }

    public String getEmail() {
        return email;
    }

    public Integer getMonthlyQuotaMinutes() {
        return monthlyQuotaMinutes;
    }

    public Integer getRemainingQuotaMinutes() {
        return remainingQuotaMinutes;
    }
}


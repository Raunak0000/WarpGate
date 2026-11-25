package com.warpgate.dto;

import com.fasterxml.jackson.annotation.JsonProperty;
import java.time.LocalDateTime;

public class UserProfileResponse {

    private String username;
    private String email;

    @JsonProperty("monthly_quota_minutes")
    private Integer monthlyQuotaMinutes;

    @JsonProperty("remaining_quota_minutes")
    private Integer remainingQuotaMinutes;

    @JsonProperty("quota_reset_at")
    private LocalDateTime quotaResetAt;

    public UserProfileResponse() {
    }

    public UserProfileResponse(String username, String email,
            Integer monthlyQuotaMinutes, Integer remainingQuotaMinutes,
            LocalDateTime quotaResetAt) {
        this.username = username;
        this.email = email;
        this.monthlyQuotaMinutes = monthlyQuotaMinutes;
        this.remainingQuotaMinutes = remainingQuotaMinutes;
        this.quotaResetAt = quotaResetAt;
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

    public LocalDateTime getQuotaResetAt() {
        return quotaResetAt;
    }
}


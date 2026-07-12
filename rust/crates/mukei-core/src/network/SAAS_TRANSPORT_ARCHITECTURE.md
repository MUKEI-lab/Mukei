# SaaS transport boundary — replay safety and secret handling

This directory belongs to the existing `mukei_core::network` logical module. It does not define business endpoints, persistence, QML/bridge behavior, entitlement rules, tenant ownership rules, or diagnostics sinks.

## Replay safety

Requests are classified semantically as safe reads, idempotent mutations, non-idempotent mutations, or streaming/download work. Automatic retry is decided from that class plus a typed transient-failure classification; the HTTP method alone is never treated as proof of replay safety.

An idempotent mutation must carry `idempotency_key` before it is sent. The request context is borrowed for the whole logical request, so `request_id`, `operation_id`, `correlation_id`, and `idempotency_key` remain unchanged across attempts. Non-idempotent mutations are sent once and are never automatically replayed by the transport. HTTP 409 is returned as a conflict for higher-level reconciliation rather than blindly retried.

Retry cost is bounded simultaneously by maximum retry attempts, maximum delay, server `Retry-After` capping, and one total logical request deadline. Permit waits, token acquisition, active I/O, body streaming, and retry sleeps all consume the same deadline. Cancellation is preserved as cancellation.

## Secret handling

The transport receives short-lived credentials only through `AccessTokenProvider`. It does not persist tokens to files, settings, SQLite, environment variables, or logs. `AccessCredential` owns secret text in `Zeroizing<String>`, does not implement `Clone`, and always redacts its `Debug` representation. Authorization header values are marked sensitive before entering reqwest.

Endpoint configuration rejects embedded URL credentials and base-URL query strings. Request diagnostics expose path-only summaries, never query strings or request bodies. Callers do not receive an arbitrary header override surface, so authorization and canonical security metadata cannot be silently replaced. Server error bodies are size-bounded, parsed into a typed envelope, control-filtered, length-bounded, and passed through the existing diagnostics sanitizer before becoming technical messages.

The normal SaaS JSON body limit is independent from existing model download streaming behavior. The existing `NetworkClientPolicy`, `build_network_client`, and retry helpers remain the compatibility surface for download and web-search consumers.

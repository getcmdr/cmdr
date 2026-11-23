ARG LISTMONK_VERSION=6.2.0

# Stage 1: Fetch default static files (public pages, assets) from the matching release.
# Our custom email templates override only the email-templates/ subdirectory.
FROM alpine:3.24@sha256:f59abe656b692d901e184be3445e084786602108599d2a51d229c0f5a48c71bf AS defaults
ARG LISTMONK_VERSION
RUN wget -qO- "https://github.com/knadh/listmonk/archive/refs/tags/v${LISTMONK_VERSION}.tar.gz" \
    | tar xz -C /tmp \
    && mv /tmp/listmonk-*/static /tmp/static

# Stage 2: Listmonk with branded email templates
FROM listmonk/listmonk:v${LISTMONK_VERSION}@sha256:f535d59e14991337a9f2d570273685378ae86b0d7698c3e00da444e3bc205286
COPY --from=defaults /tmp/static /listmonk/custom-static
COPY email-templates/ /listmonk/custom-static/email-templates/

FROM alpine:3.24@sha256:f59abe656b692d901e184be3445e084786602108599d2a51d229c0f5a48c71bf
RUN apk add --no-cache samba iproute2
COPY smb.conf /etc/samba/smb.conf
COPY entrypoint.sh /entrypoint.sh
RUN chmod +x /entrypoint.sh \
    && mkdir -p /shares/public \
    && echo "Slow server sample file" > /shares/public/sample.txt \
    && echo "Another file for latency testing" > /shares/public/data.txt \
    && chmod -R 777 /shares/public
EXPOSE 445
HEALTHCHECK --interval=2s --timeout=5s --retries=15 \
    CMD nc -z localhost 445
CMD ["/entrypoint.sh"]

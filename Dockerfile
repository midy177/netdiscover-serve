FROM alpine:3.22

ARG TARGETARCH

RUN apk add --no-cache ca-certificates

COPY dist/docker/netdiscover-serve-${TARGETARCH} /usr/local/bin/netdiscover-serve
RUN chmod +x /usr/local/bin/netdiscover-serve

EXPOSE 8080/tcp
EXPOSE 8080/udp

ENTRYPOINT ["netdiscover-serve"]
CMD ["-serve"]

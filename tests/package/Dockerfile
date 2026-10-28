ARG DEBIAN_RELEASE=trixie
FROM debian:${DEBIAN_RELEASE}

# logrotate is recommended by the package, tzdata provides the system timezone
# read by the gateway.
# hadolint ignore=DL3008
RUN apt-get update \
    && DEBIAN_FRONTEND=noninteractive apt-get install --yes --no-install-recommends \
        logrotate \
        procps \
        systemd \
        tzdata \
    && rm -rf /var/lib/apt/lists/*

RUN ln -sf /usr/share/zoneinfo/Europe/Paris /etc/localtime

COPY --from=bats/bats:1.14.0 /opt/bats /opt/bats
COPY --from=bats/bats:1.14.0 /usr/lib/bats /usr/lib/bats

CMD ["/usr/lib/systemd/systemd"]

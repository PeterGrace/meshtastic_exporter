FROM docker.io/ubuntu:20.04
ARG TARGETARCH

RUN mkdir -p /opt/app
WORKDIR /opt/app
COPY ./tools/target_arch.sh /opt/app
COPY config.yaml /opt/app/config.yaml
RUN --mount=type=bind,target=/context \
 cp /context/target/$(/opt/app/target_arch.sh)/release/meshtastic_exporter /opt/app/meshtastic_exporter
CMD ["/opt/app/meshtastic_exporter"]
EXPOSE 8443

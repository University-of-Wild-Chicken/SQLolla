FROM ubuntu:26.04

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates libgmp10 libstdc++6 zlib1g \
    && rm -rf /var/lib/apt/lists/* \
    && install -d -o 10001 -g 10001 /var/lib/sqlolla

COPY . /opt/sqlolla
WORKDIR /opt/sqlolla
USER 10001:10001
ENV SQLolla_STATE=/var/lib/sqlolla
ENTRYPOINT ["/opt/sqlolla/start-sqlolla.sh"]

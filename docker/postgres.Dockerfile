FROM postgres:15-alpine

COPY docker-entrypoint-run-migrations.sh /usr/local/bin/docker-entrypoint-run-migrations.sh

RUN sed -i '1s/^\xEF\xBB\xBF//' /usr/local/bin/docker-entrypoint-run-migrations.sh \
    && sed -i 's/\r$//' /usr/local/bin/docker-entrypoint-run-migrations.sh \
    && chmod +x /usr/local/bin/docker-entrypoint-run-migrations.sh

ENTRYPOINT ["sh", "/usr/local/bin/docker-entrypoint-run-migrations.sh"]
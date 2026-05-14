FROM postgres:15-alpine

# Copy the startup wrapper that launches Postgres, waits for readiness
# then executes any SQL files in /migrations before keeping Postgres running.
COPY docker-entrypoint-run-migrations.sh /usr/local/bin/docker-entrypoint-run-migrations.sh
# Ensure script has Unix line endings and is executable (fixes exec format error from CRLF)
RUN sed -i 's/\r$//' /usr/local/bin/docker-entrypoint-run-migrations.sh \
	&& chmod +x /usr/local/bin/docker-entrypoint-run-migrations.sh

# Run the script with sh to avoid relying on the shebang being interpreted by the kernel
ENTRYPOINT ["sh", "/usr/local/bin/docker-entrypoint-run-migrations.sh"]


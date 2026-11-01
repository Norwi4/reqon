FROM node:22-alpine
WORKDIR /app
COPY --chown=node:node package.json server.mjs ./
COPY --chown=node:node src ./src
COPY --chown=node:node public ./public
RUN mkdir -p /app/.cache /srv/releases && chown -R node:node /app/.cache /srv/releases
USER node
ENV HOST=0.0.0.0 PORT=4173 RELEASES_DIR=/srv/releases
EXPOSE 4173
HEALTHCHECK --interval=30s --timeout=5s --start-period=15s --retries=3 CMD node -e "fetch('http://127.0.0.1:4173/healthz').then(r=>process.exit(r.ok?0:1)).catch(()=>process.exit(1))"
CMD ["sh", "-c", "node src/build.mjs && exec node server.mjs"]

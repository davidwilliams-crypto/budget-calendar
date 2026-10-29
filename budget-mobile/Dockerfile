FROM node:24.19.0-bookworm-slim
WORKDIR /app
# No npm install: the server uses Node's built-in SQLite and standard library only.
COPY --chown=node:node package.json ./
COPY --chown=node:node server/*.mjs ./server/
COPY --chown=node:node public/*.mjs public/*.js public/*.html public/*.css public/*.webmanifest public/*.png public/*.wasm ./public/
ENV NODE_ENV=production BUDGET_HOST=0.0.0.0 PORT=8787 BUDGET_DATA_DIR=/data
RUN mkdir -p /data && chown node:node /data
USER node
VOLUME ["/data"]
EXPOSE 8787
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s CMD node -e "fetch('http://127.0.0.1:8787/health').then(r=>{if(!r.ok)process.exit(1)}).catch(()=>process.exit(1))"
CMD ["node", "server/start.mjs"]

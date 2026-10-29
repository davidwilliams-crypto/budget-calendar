import { fileURLToPath } from 'node:url';
import { createBudgetServer } from './server.mjs';

const port = Number(process.env.PORT ?? '8787');
if (!Number.isInteger(port) || port < 1 || port > 65535) throw new Error('PORT must be between 1 and 65535');
const host = process.env.BUDGET_HOST ?? '127.0.0.1';
const app = createBudgetServer({
  dataDir: process.env.BUDGET_DATA_DIR,
  publicDir: fileURLToPath(new URL('../public/', import.meta.url)),
  setupToken: process.env.BUDGET_SETUP_TOKEN,
  publicOrigin: process.env.BUDGET_PUBLIC_ORIGIN,
  development: process.env.BUDGET_DEVELOPMENT === '1',
  trustedProxy: process.env.BUDGET_TRUSTED_PROXY ?? 'none',
  renderEnvironment: process.env.RENDER === 'true',
  ...(process.env.BUDGET_MAX_PAYLOAD_BYTES ? { maxPayloadBytes: Number(process.env.BUDGET_MAX_PAYLOAD_BYTES) } : {}),
});
app.server.listen(port, host, () => process.stdout.write(`Budget private service listening on ${host}:${port}\n`));
for (const signal of ['SIGINT', 'SIGTERM']) process.on(signal, () => { app.close().then(() => process.exit(0)); });

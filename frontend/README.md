# ffplayout Frontend

The ffplayout web interface is built with Vue, Vite, Tailwind CSS, and
TypeScript. Its source is in this directory, while the Node.js project files
and npm scripts are kept in the repository root.

## Requirements

- A current Node.js LTS release with npm.
- A running ffplayout backend for local development. By default, the Vite
  development server proxies API requests to `http://127.0.0.1:8787`.

## Install Dependencies

Run the commands from the repository root:

```bash
npm ci
```

Use `npm install` only when intentionally updating dependencies and
`package-lock.json`.

## Development

Start the backend in one terminal:

```bash
cargo run -- -l 127.0.0.1:8787
```

Start the Vite development server from the repository root in a second
terminal:

```bash
npm run dev
```

Open `http://127.0.0.1:5757`. On a new installation, complete the first-time
setup to create the global settings and initial global admin.

## Build and Checks

Create the production bundle in `frontend/dist`:

```bash
npm run build
```

Run the TypeScript check:

```bash
npm run type-check
```

Run the linter. The current lint command applies automatic fixes:

```bash
npm run lint
```

## Further Documentation

See the [developer documentation](../docs/developer.md) for backend setup,
generated frontend types, packaging, and the full development workflow.

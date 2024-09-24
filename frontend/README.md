ffplayout-frontend
=====

This web application is used to manage the [ffplayout](https://github.com/ffplayout/ffplayout).

The interface is mostly designed for 24/7 streaming. Other scenarios like streaming in folder mode or playlists with no start time will work but will not be displayed correctly.

For a better understanding of the functionality, take a look at the screenshots below.

### Login
![login](/docs/images/login.png)

### System Dashboard
![login](/docs/images/dasboard.png)

### Control Page
![player](/docs/images/player.png)

### Media Page
![media](/docs/images/media.png)

### Message Page
![message](/docs/images/message.png)

### Logging Page
![logging](/docs/images/logging.png)

### Configuration Page
![config-gui](/docs/images/config-gui.png)

## Setup

Make sure to install the dependencies:

```bash
# yarn
yarn install

# npm
npm install

# pnpm
pnpm install --shamefully-hoist
```

## Development Server

Start the development server on http://localhost:3000

```bash
npm run dev
```

## Production

Build the application for production:

```bash
npm run build
```

Locally preview production build:

```bash
npm run preview
```

Check out the [deployment documentation](https://nuxt.com/docs/getting-started/deployment) for more information.

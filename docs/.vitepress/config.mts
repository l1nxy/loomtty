import { defineConfig } from 'vitepress'

const base = '/loomtty/'

// https://vitepress.dev/reference/site-config
export default defineConfig({
  base,
  lang: 'en-US',
  title: 'loomtty',
  description:
    'A GPU-accelerated terminal multiplexer written in Rust — column-based layouts and workspaces, a built-in browser client, and remote attach over a binary protocol.',

  lastUpdated: true,
  cleanUrls: true,
  ignoreDeadLinks: true,

  head: [
    ['link', { rel: 'icon', type: 'image/svg+xml', href: `${base}logo.svg` }],
    ['meta', { name: 'theme-color', content: '#007AFF' }],
    ['meta', { property: 'og:type', content: 'website' }],
    ['meta', { property: 'og:title', content: 'loomtty' }],
    [
      'meta',
      {
        property: 'og:description',
        content: 'A GPU-accelerated terminal multiplexer written in Rust.',
      },
    ],
  ],

  themeConfig: {
    // https://vitepress.dev/reference/default-theme-config
    logo: '/logo.svg',

    nav: [
      { text: 'Guide', link: '/guide/', activeMatch: '/guide/' },
      { text: 'Reference', link: '/reference/cli', activeMatch: '/reference/' },
      {
        text: 'v0.1',
        items: [
          {
            text: 'Releases',
            link: 'https://github.com/l1nxy/loomtty/releases',
          },
          {
            text: 'Changelog',
            link: 'https://github.com/l1nxy/loomtty/commits/dev',
          },
        ],
      },
    ],

    sidebar: {
      '/guide/': [
        {
          text: 'Introduction',
          collapsed: false,
          items: [
            { text: 'What is loomtty?', link: '/guide/' },
            { text: 'Installation', link: '/guide/installation' },
            { text: 'Quick Start', link: '/guide/quick-start' },
          ],
        },
        {
          text: 'Using loomtty',
          collapsed: false,
          items: [
            { text: 'Layout & Workspaces', link: '/guide/layout' },
            { text: 'Keybindings', link: '/guide/keybindings' },
            { text: 'Configuration', link: '/guide/configuration' },
            { text: 'Theming', link: '/guide/theming' },
            { text: 'Shell Integration', link: '/guide/shell-integration' },
            { text: 'Remote & Predictive Echo', link: '/guide/remote-attach' },
            { text: 'Web UI', link: '/guide/web-ui' },
            { text: 'System Tray', link: '/guide/system-tray' },
          ],
        },
        {
          text: 'Extending',
          collapsed: false,
          items: [{ text: 'Plugins', link: '/guide/plugins' }],
        },
      ],
      '/reference/': [
        {
          text: 'Reference',
          items: [
            { text: 'CLI', link: '/reference/cli' },
            { text: 'Configuration', link: '/reference/configuration' },
            { text: 'Actions', link: '/reference/actions' },
          ],
        },
      ],
    },

    socialLinks: [
      { icon: 'github', link: 'https://github.com/l1nxy/loomtty' },
    ],

    search: {
      provider: 'local',
    },

    editLink: {
      pattern: 'https://github.com/l1nxy/loomtty/edit/dev/docs/:path',
      text: 'Edit this page on GitHub',
    },

    footer: {
      message: 'Released under the AGPL-3.0-only License.',
      copyright: 'Copyright © 2025–present loomtty contributors',
    },
  },
})

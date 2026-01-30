import {themes as prismThemes} from 'prism-react-renderer';
import type {Config} from '@docusaurus/types';
import type * as Preset from '@docusaurus/preset-classic';

const config: Config = {
  title: 'Omen',
  tagline: 'Multi-language code analysis for AI-assisted development',
  favicon: 'img/favicon.ico',

  future: {
    v4: true,
  },

  url: 'https://panbanda.github.io',
  baseUrl: '/omen/',

  organizationName: 'panbanda',
  projectName: 'omen',

  onBrokenLinks: 'throw',

  i18n: {
    defaultLocale: 'en',
    locales: ['en'],
  },

  presets: [
    [
      'classic',
      {
        docs: {
          sidebarPath: './sidebars.ts',
          editUrl: 'https://github.com/panbanda/omen/tree/gh-pages/',
        },
        blog: false,
        theme: {
          customCss: './src/css/custom.css',
        },
      } satisfies Preset.Options,
    ],
  ],

  themeConfig: {
    image: 'img/omen-social-card.jpg',
    colorMode: {
      respectPrefersColorScheme: true,
    },
    navbar: {
      title: 'Omen',
      items: [
        {
          type: 'docSidebar',
          sidebarId: 'docsSidebar',
          position: 'left',
          label: 'Docs',
        },
        {
          href: 'https://crates.io/crates/omen-cli',
          label: 'Crates.io',
          position: 'right',
        },
        {
          href: 'https://github.com/panbanda/omen',
          label: 'GitHub',
          position: 'right',
        },
      ],
    },
    footer: {
      style: 'dark',
      links: [
        {
          title: 'Documentation',
          items: [
            {label: 'Getting Started', to: '/docs/getting-started'},
            {label: 'Analyzers', to: '/docs/analyzers/overview'},
            {label: 'Configuration', to: '/docs/configuration'},
          ],
        },
        {
          title: 'Integrations',
          items: [
            {label: 'MCP Server', to: '/docs/integrations/mcp-server'},
            {label: 'CI/CD', to: '/docs/integrations/ci-cd'},
            {label: 'Claude Code Plugin', to: '/docs/integrations/claude-code-plugin'},
          ],
        },
        {
          title: 'More',
          items: [
            {label: 'GitHub', href: 'https://github.com/panbanda/omen'},
            {label: 'Crates.io', href: 'https://crates.io/crates/omen-cli'},
          ],
        },
      ],
      copyright: `Copyright ${new Date().getFullYear()} Panbanda. Apache License 2.0.`,
    },
    prism: {
      theme: prismThemes.github,
      darkTheme: prismThemes.dracula,
      additionalLanguages: ['bash', 'toml', 'rust', 'json', 'yaml'],
    },
  } satisfies Preset.ThemeConfig,
};

export default config;

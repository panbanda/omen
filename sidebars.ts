import type {SidebarsConfig} from '@docusaurus/plugin-content-docs';

const sidebars: SidebarsConfig = {
  docsSidebar: [
    'intro',
    'getting-started',
    {
      type: 'category',
      label: 'Analyzers',
      collapsed: false,
      items: [
        'analyzers/overview',
        {
          type: 'category',
          label: 'Complexity & Quality',
          items: [
            'analyzers/complexity',
            'analyzers/cohesion',
            'analyzers/tdg',
            'analyzers/smells',
          ],
        },
        {
          type: 'category',
          label: 'Risk & Defects',
          items: [
            'analyzers/defect-prediction',
            'analyzers/change-risk',
            'analyzers/diff-analysis',
            'analyzers/hotspots',
          ],
        },
        {
          type: 'category',
          label: 'History & Ownership',
          items: [
            'analyzers/churn',
            'analyzers/temporal-coupling',
            'analyzers/ownership',
          ],
        },
        {
          type: 'category',
          label: 'Structure & Dependencies',
          items: [
            'analyzers/dependency-graph',
            'analyzers/dead-code',
            'analyzers/code-clones',
            'analyzers/repomap',
          ],
        },
        {
          type: 'category',
          label: 'Specialized',
          items: [
            'analyzers/satd',
            'analyzers/feature-flags',
            'analyzers/mutation-testing',
          ],
        },
      ],
    },
    'repository-score',
    'semantic-search',
    'configuration',
    {
      type: 'category',
      label: 'Integrations',
      items: [
        'integrations/mcp-server',
        'integrations/ci-cd',
        'integrations/claude-code-plugin',
      ],
    },
    'output-formats',
    'supported-languages',
    'research',
  ],
};

export default sidebars;

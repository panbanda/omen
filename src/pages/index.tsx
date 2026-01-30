import clsx from 'clsx';
import useDocusaurusContext from '@docusaurus/useDocusaurusContext';
import Layout from '@theme/Layout';
import Heading from '@theme/Heading';
import Link from '@docusaurus/Link';

import styles from './index.module.css';

function HomepageHeader() {
  const {siteConfig} = useDocusaurusContext();
  return (
    <header className={clsx('hero hero--primary', styles.heroBanner)}>
      <div className="container">
        <Heading as="h1" className="hero__title">
          {siteConfig.title}
        </Heading>
        <p className="hero__subtitle">{siteConfig.tagline}</p>
        <div className={styles.buttons}>
          <Link
            className="button button--secondary button--lg"
            to="/docs/getting-started">
            Get Started
          </Link>
          <Link
            className="button button--secondary button--lg"
            style={{marginLeft: '1rem'}}
            to="/docs/analyzers/overview">
            Analyzers
          </Link>
        </div>
      </div>
    </header>
  );
}

const features = [
  {
    title: '19 Analyzers',
    description:
      'Complexity, technical debt, code clones, defect prediction, dependency graphs, hotspots, mutation testing, and more. All backed by peer-reviewed research.',
  },
  {
    title: '13 Languages',
    description:
      'Go, Rust, Python, TypeScript, JavaScript, TSX/JSX, Java, C, C++, C#, Ruby, PHP, and Bash via tree-sitter parsing.',
  },
  {
    title: 'Built for AI',
    description:
      'MCP server integration, TOON output format for token efficiency, semantic search, and repository maps designed for LLM context windows.',
  },
  {
    title: 'Research-Backed',
    description:
      'Every analyzer is grounded in published software engineering research: McCabe complexity, Chidamber-Kemerer metrics, Kamei JIT prediction, and more.',
  },
  {
    title: 'Fast & Parallel',
    description:
      'Built in Rust with rayon parallelism, sparse PageRank algorithms, and incremental caching. Handles large monorepos efficiently.',
  },
  {
    title: 'Remote Repos',
    description:
      'Analyze any public GitHub repository without cloning. Just pass owner/repo and Omen handles the rest.',
  },
];

function Feature({title, description}: {title: string; description: string}) {
  return (
    <div className={clsx('col col--4')}>
      <div className="text--center padding-horiz--md padding--md">
        <Heading as="h3">{title}</Heading>
        <p>{description}</p>
      </div>
    </div>
  );
}

export default function Home(): React.JSX.Element {
  const {siteConfig} = useDocusaurusContext();
  return (
    <Layout
      title={siteConfig.title}
      description="Multi-language code analysis CLI for AI-assisted development">
      <HomepageHeader />
      <main>
        <section className={styles.features}>
          <div className="container">
            <div className="row">
              {features.map((props, idx) => (
                <Feature key={idx} {...props} />
              ))}
            </div>
          </div>
        </section>
      </main>
    </Layout>
  );
}

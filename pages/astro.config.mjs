// @ts-check
import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';

export default defineConfig({
	site: 'https://proofofbots.github.io',
	base: '/web-re-toolkit',
	integrations: [
		starlight({
			title: 'web-re-toolkit',
			description:
				'A Rust toolkit for reverse engineering client-side web protections, and the headless clients built with it.',
			social: [
				{
					icon: 'github',
					label: 'GitHub',
					href: 'https://github.com/proofofbots/web-re-toolkit',
				},
				{
					icon: 'discord',
					label: 'Discord',
					href: 'https://discord.gg/nbBePnsa9',
				},
			],
			editLink: {
				baseUrl: 'https://github.com/proofofbots/web-re-toolkit/edit/main/pages/',
			},
			lastUpdated: true,
			customCss: ['./src/styles/theme.css'],
			sidebar: [
				{
					label: 'Start here',
					items: [
						{ label: 'Install', slug: 'start/install' },
						{ label: 'Workspace layout', slug: 'start/workspace' },
						{ label: 'A worked pass', slug: 'start/first-pass' },
					],
				},
				{
					label: 'Concepts',
					items: [
						{ label: 'Core ideas', slug: 'concepts/core-ideas' },
						{ label: 'Limitations', slug: 'concepts/limitations' },
					],
				},
				{
					label: 'Guides',
					items: [
						{ label: 'Finding things again after a rebuild', slug: 'guides/identification' },
						{ label: 'The browser surface', slug: 'guides/sandbox' },
						{ label: 'Headless clients', slug: 'guides/clients' },
						{ label: 'The Akamai client', slug: 'guides/akamai' },
						{ label: 'The Kasada client', slug: 'guides/kasada' },
					],
				},
				{
					label: 'Packages',
					items: [
						{ label: 'Overview', slug: 'packages' },
						{ label: 'Node.js', slug: 'packages/node' },
						{ label: 'Python', slug: 'packages/python' },
						{ label: 'Go', slug: 'packages/go' },
						{ label: 'Rust', slug: 'packages/rust' },
					],
				},
				{
					label: 'Reference',
					items: [
						{ label: 'Command reference', slug: 'reference/cli' },
						{ label: 'Crates', slug: 'reference/crates' },
						{ label: 'The sidecar protocol', slug: 'reference/protocol' },
					],
				},
				{
					label: 'Research',
					items: [{ label: 'ALTCHA', slug: 'research/altcha' }],
				},
			],
		}),
	],
});

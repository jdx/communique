import { defineConfig } from "vitepress";

import spec from "../cli/commands.json";

interface Cmd {
  name: string;
  full_cmd: string[];
  subcommands: Record<string, Cmd>;
  hide?: boolean;
}

function getCommands(cmd: Cmd): string[][] {
  const commands: string[][] = [];
  for (const [name, sub] of Object.entries(cmd.subcommands)) {
    if (sub.hide) continue;
    commands.push(sub.full_cmd);
    commands.push(...getCommands(sub));
  }
  return commands;
}

const commands = getCommands(spec.cmd);

export default defineConfig({
  title: "communiqué",
  description: "Editorialized release notes powered by AI",
  appearance: "force-dark",
  cleanUrls: true,
  lastUpdated: true,

  head: [
    ["meta", { name: "theme-color", content: "#b967ff" }],
    [
      "meta",
      { property: "og:title", content: "communiqué" },
    ],
    [
      "meta",
      {
        property: "og:description",
        content: "Editorialized release notes powered by AI",
      },
    ],
    ["meta", { property: "og:type", content: "website" }],
    ["meta", { name: "twitter:card", content: "summary" }],
  ],

  themeConfig: {
    nav: [
      { text: "Guide", link: "/guide/getting-started" },
      { text: "CLI Reference", link: "/cli/" },
    ],

    sidebar: [
      {
        text: "Guide",
        items: [
          { text: "Getting Started", link: "/guide/getting-started" },
          { text: "Configuration", link: "/guide/configuration" },
        ],
      },
      {
        text: "CLI Reference",
        link: "/cli/",
        collapsed: true,
        items: commands.map((cmd) => ({
          text: cmd.join(" "),
          link: `/cli/${cmd.join("/")}`,
        })),
      },
    ],

    socialLinks: [
      { icon: "github", link: "https://github.com/jdx/communique" },
    ],

    editLink: {
      pattern:
        "https://github.com/jdx/communique/edit/main/docs/:path",
      text: "Edit this page on GitHub",
    },

    search: {
      provider: "local",
    },

    footer: {
      message: "Thank you for visiting the communiqué information superhighway ── MIT Licensed",
      copyright: "&copy; 2026 Jeff Dickey ── A better tomorrow, today™",
    },
  },
});

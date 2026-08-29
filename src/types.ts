export type TokenValues = {
  input: number;
  output: number;
  cacheRead: number;
  cacheWrite: number;
  reasoning: number;
};

export type UsageBucket = TokenValues & {
  start: string;
  end: string;
};

export type RateLimit = {
  usedPercent: number;
  resetsAt: number | null;
  source: string;
};

export type ToolUsage = {
  fiveHour: RateLimit | null;
  weekly: RateLimit | null;
  buckets: UsageBucket[];
};

export type DashboardData = {
  codex: ToolUsage;
  claude: ToolUsage;
  generatedAt: string;
  warnings: string[];
};

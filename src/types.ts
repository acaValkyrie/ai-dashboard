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

export type AntigravityGroup = {
  name: string;
  fiveHour: RateLimit | null;
  weekly: RateLimit | null;
};

export type AntigravityUsage = {
  groups: AntigravityGroup[];
};

export type DashboardData = {
  codex: ToolUsage;
  claude: ToolUsage;
  /** Antigravity CLI(agy)が見つからない環境では null */
  antigravity: AntigravityUsage | null;
  claudeLoginRequired: boolean;
  generatedAt: string;
  warnings: string[];
};

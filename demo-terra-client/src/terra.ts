export interface PropertyValueRes {
  property: string;
  value: unknown;
  context: TxContext;
}

export interface TxContext {
  tx_id: string;
  branch: string;
  reasoning?: string;
  time?: string;
}

export interface EntityRes {
  slug: string;
  description?: unknown;
  properties: PropertyValueRes[];
  meta: Record<string, unknown>;
  context: TxContext;
}

export interface TransactionRes {
  meta: Record<string, unknown>;
  context: TxContext;
}

export interface BranchRes {
  slug: string;
  parent: string;
  meta: Record<string, unknown>;
  context: TxContext;
}

export interface ManagedRes {
  type_name: string;
  slug: string;
  state?: string;
  fields: Record<string, unknown>;
  context: TxContext;
}

export interface SimilarEntityRes {
  slug: string;
  similarity: number;
}

export interface EntityReq {
  slug: string;
  description?: unknown;
  properties?: { property: string; value: unknown }[];
  meta?: Record<string, unknown>;
}

export interface ManagedReq {
  type_name: string;
  slug: string;
  state?: string;
  fields?: Record<string, unknown>;
}

export interface TouchReq {
  entity: string;
  reasoning: string;
}

export interface DeleteReq {
  entity: string;
  reasoning: unknown;
}

export interface TransactionReq {
  meta: Record<string, unknown>;
  create?: EntityReq[];
  update?: EntityReq[];
  delete?: DeleteReq[];
  create_managed?: ManagedReq[];
  update_managed?: ManagedReq[];
  touch?: TouchReq[];
}

export interface CheckoutReq {
  slug: string;
  meta: Record<string, unknown>;
  created_from_tx?: string;
  transaction: Omit<TransactionReq, "meta"> & { meta: Record<string, unknown> };
}

export interface CheckoutRes {
  branch: string;
  created_from_tx: string;
  transaction: TransactionRes;
}

export class TerraClient {
  constructor(private baseUrl: string) {}

  async transaction(branch: string, req: TransactionReq): Promise<TransactionRes> {
    return this.query<TransactionRes>({ command: "transaction", branch, ...req });
  }

  async checkout(branch: string, req: CheckoutReq): Promise<CheckoutRes> {
    return this.query<CheckoutRes>({ command: "checkout", branch, ...req });
  }

  async touchedEntities(branch: string, limit: number, atTx?: string): Promise<EntityRes[]> {
    return this.query<EntityRes[]>({
      command: "entities.touched", branch, limit,
      ...(atTx && { at_tx: atTx }),
    });
  }

  async listTransactions(branch: string, limit: number, atTx?: string): Promise<TransactionRes[]> {
    return this.query<TransactionRes[]>({
      command: "transactions.list", branch, limit,
      ...(atTx && { at_tx: atTx }),
    });
  }

  async getBranch(branch: string): Promise<BranchRes> {
    return this.query<BranchRes>({ command: "branch.get", branch });
  }

  async listManaged(branch: string): Promise<ManagedRes[]> {
    return this.query<ManagedRes[]>({ command: "managed.list", branch });
  }

  async similarEntities(branch: string, queries: unknown[], limit: number, minSimilarity?: number): Promise<SimilarEntityRes[]> {
    return this.query<SimilarEntityRes[]>({
      command: "entities.similar",
      branch,
      queries,
      limit,
      ...(minSimilarity != null && { min_similarity: minSimilarity }),
    });
  }

  private async query<T>(body: Record<string, unknown>): Promise<T> {
    const res = await fetch(`${this.baseUrl}/query`, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        Accept: "application/json",
      },
      body: JSON.stringify(body),
    });

    if (!res.ok) {
      let errBody: { error: string; kind: string };
      try {
        errBody = await res.json() as { error: string; kind: string };
      } catch {
        throw new TerraError(res.statusText || "Request failed", "http_error", res.status);
      }
      throw new TerraError(errBody.error, errBody.kind, res.status);
    }

    return await res.json() as T;
  }
}

export class TerraError extends Error {
  constructor(
    message: string,
    public readonly kind: string,
    public readonly status: number,
  ) {
    super(message);
    this.name = "TerraError";
  }
}

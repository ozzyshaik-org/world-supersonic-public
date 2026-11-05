export interface UpstreamRequest {
  method: "GET";
  path: string;
  rawQuery: string;
}

export interface UpstreamResponse {
  status: number;
  body: unknown;
  contentType: string;
}

export interface UpstreamClient {
  readonly name: string;
  fetch(req: UpstreamRequest): Promise<UpstreamResponse>;
}

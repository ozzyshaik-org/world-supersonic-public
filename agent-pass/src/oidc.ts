import * as client from "openid-client";
import type { AuthorizationInput, OidcProvider, RedeemInput, ValidatedIdentity } from "./types.js";
import { ORB_ACR } from "./types.js";

export interface LiveOidcOptions {
  issuer: string;
  clientId: string;
  clientSecret: string;
  clientAuthMethod: "client_secret_basic" | "client_secret_post";
}

/**
 * Live World IdP adapter. Standard signature/issuer/audience/expiry, state,
 * one-time code, S256 PKCE, nonce and auth_time validation are delegated to
 * openid-client. Application assurance and operation checks remain in AgentPassService.
 */
export class WorldOidcProvider implements OidcProvider {
  private configuration?: client.Configuration;

  constructor(private readonly options: LiveOidcOptions) {}

  private async config(): Promise<client.Configuration> {
    if (!this.configuration) {
      const auth = this.options.clientAuthMethod === "client_secret_post"
        ? client.ClientSecretPost()
        : client.ClientSecretBasic();
      this.configuration = await client.discovery(
        new URL(this.options.issuer),
        this.options.clientId,
        this.options.clientSecret,
        auth,
      );
    }
    return this.configuration;
  }

  async authorizationUrl(input: AuthorizationInput): Promise<string> {
    const configuration = await this.config();
    const codeChallenge = await client.calculatePKCECodeChallenge(input.codeVerifier);
    const url = client.buildAuthorizationUrl(configuration, {
      redirect_uri: input.redirectUri,
      scope: "openid",
      state: input.state,
      nonce: input.nonce,
      code_challenge: codeChallenge,
      code_challenge_method: "S256",
      max_age: "0",
      claims: JSON.stringify({ id_token: { acr: { essential: true, values: [ORB_ACR] } } }),
    });
    return url.href;
  }

  async redeem(input: RedeemInput): Promise<ValidatedIdentity> {
    const configuration = await this.config();
    const tokens = await client.authorizationCodeGrant(configuration, new URL(input.callbackUrl), {
      pkceCodeVerifier: input.codeVerifier,
      expectedState: input.state,
      expectedNonce: input.nonce,
      idTokenExpected: true,
      maxAge: input.maxAgeSeconds,
    });
    const claims = tokens.claims();
    if (!claims) throw new Error("OIDC ID token claims unavailable");
    return {
      iss: String(claims.iss),
      sub: String(claims.sub),
      auth_time: Number(claims.auth_time),
      acr: claims.acr,
      amr: claims.amr,
    };
    // tokens (including ID/access tokens) deliberately leave scope here and are never stored.
  }
}

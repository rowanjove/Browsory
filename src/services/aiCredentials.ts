export type AiCredentialScope = 'chat' | 'embedding';
export const DEFAULT_AI_ENDPOINT = 'https://api.openai.com/v1';

/**
 * Must stay in sync with ai::http::credential_ref_for_endpoint.
 * The digest is a namespace identifier only; the secret value remains in the
 * OS-backed store. WebCrypto keeps endpoint text out of the credential key.
 */
export const credentialRefForEndpoint = async (
  baseUrl: string,
  scope: AiCredentialScope = 'chat',
): Promise<string> => {
  const normalized = baseUrl.trim().replace(/\/+$/, '');
  const digest = await globalThis.crypto.subtle.digest(
    'SHA-256',
    new TextEncoder().encode(normalized),
  );
  const hex = Array.from(new Uint8Array(digest), (byte) => byte.toString(16).padStart(2, '0')).join('');
  const prefix = scope === 'embedding' ? 'ai_embedding_credential_' : 'ai_credential_';
  return `${prefix}${hex}`;
};

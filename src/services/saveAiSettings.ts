export type AiSettingsField = 'credential' | 'endpoint' | 'model';

export class AiSettingsSaveError extends Error {
  constructor(
    readonly confirmedFields: AiSettingsField[],
    readonly failedField: AiSettingsField,
    readonly reason: unknown,
  ) {
    super('AI settings were not fully saved');
    this.name = 'AiSettingsSaveError';
  }
}

// SQL settings and the OS credential store are separate resources. Report which
// writes were acknowledged instead of claiming a rollback that did not happen.
export const saveAiSettings = async (
  writeSetting: (key: string, value: string) => Promise<void>,
  settings: { credentialRef: string; apiKey: string; baseUrl: string; model: string; embedding?: boolean },
): Promise<void> => {
  const prefix = settings.embedding ? 'ai_embedding_' : 'ai_';
  const writes: [AiSettingsField, string, string][] = [
    ['credential', settings.credentialRef, settings.apiKey],
    ['endpoint', `${prefix}base_url`, settings.baseUrl],
    ['model', `${prefix}model`, settings.model],
  ];
  const confirmed: AiSettingsField[] = [];
  for (const [field, key, value] of writes) {
    try {
      await writeSetting(key, value);
      confirmed.push(field);
    } catch (error) {
      throw new AiSettingsSaveError([...confirmed], field, error);
    }
  }
};

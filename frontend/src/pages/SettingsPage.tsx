import { faGear } from '@fortawesome/free-solid-svg-icons';
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { useEffect, useState } from 'react';
import { httpErrorToHuman } from '@/api/axios.ts';
import Button from '@/elements/buttons/Button.tsx';
import TitleCard from '@/elements/data-display/TitleCard.tsx';
import Alert from '@/elements/feedback/Alert.tsx';
import Spinner from '@/elements/feedback/Spinner.tsx';
import NumberInput from '@/elements/input/NumberInput.tsx';
import Switch from '@/elements/input/Switch.tsx';
import Group from '@/elements/layout/Group.tsx';
import Stack from '@/elements/layout/Stack.tsx';
import Text from '@/elements/typography/Text.tsx';
import { useAdminCan } from '@/plugins/usePermissions.ts';
import { useToast } from '@/providers/ToastProvider.tsx';
import { getSettings, PACKAGE, type Settings, updateSettings } from '../api.ts';
import { useExtTranslations } from '../translations.ts';

const settingsKey = [PACKAGE, 'settings'] as const;

export default function SettingsPage() {
  const { t } = useExtTranslations();
  const { addToast } = useToast();
  const canManage = useAdminCan('extensions.manage');
  const queryClient = useQueryClient();
  const query = useQuery({ queryKey: settingsKey, queryFn: getSettings });
  const [form, setForm] = useState<Settings | null>(null);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    if (query.data) setForm(query.data);
  }, [query.data]);

  const toNumber = (val: string | number, fallback: number): number => {
    const parsed = typeof val === 'number' ? val : Number.parseInt(val, 10);
    return Number.isFinite(parsed) ? parsed : fallback;
  };

  const save = async () => {
    if (!form) return;
    setSaving(true);
    try {
      const saved = await updateSettings(form);
      setForm(saved);
      queryClient.setQueryData(settingsKey, saved);
      addToast(t('settings.saved', {}), 'success');
    } catch (error) {
      addToast(httpErrorToHuman(error), 'error');
    } finally {
      setSaving(false);
    }
  };

  return (
    <TitleCard title={t('settings.title', {})} icon={<FontAwesomeIcon icon={faGear} />}>
      {query.isLoading ? (
        <Group justify='center' py='md'>
          <Spinner />
        </Group>
      ) : query.error || !form ? (
        <Alert color='red'>{httpErrorToHuman(query.error)}</Alert>
      ) : (
        <Stack gap='md'>
          <Text size='sm' c='dimmed'>
            {t('settings.description', {})}
          </Text>

          <div className='grid grid-cols-1 md:grid-cols-2 gap-4'>
            <NumberInput
              label={t('settings.maxConcurrentImports', {})}
              description={t('settings.maxConcurrentImportsDescription', {})}
              min={1}
              max={32}
              value={form.max_concurrent_imports}
              onChange={(v) => setForm({ ...form, max_concurrent_imports: toNumber(v, form.max_concurrent_imports) })}
              disabled={!canManage}
            />
            <NumberInput
              label={t('settings.connectionTimeoutSecs', {})}
              description={t('settings.connectionTimeoutSecsDescription', {})}
              min={5}
              max={120}
              value={form.connection_timeout_secs}
              onChange={(v) => setForm({ ...form, connection_timeout_secs: toNumber(v, form.connection_timeout_secs) })}
              disabled={!canManage}
            />
            <NumberInput
              label={t('settings.maxImportSizeMb', {})}
              description={t('settings.maxImportSizeMbDescription', {})}
              min={0}
              max={10485760}
              value={form.max_import_size_mb}
              onChange={(v) => setForm({ ...form, max_import_size_mb: toNumber(v, form.max_import_size_mb) })}
              disabled={!canManage}
            />
            <NumberInput
              label={t('settings.maxImportFiles', {})}
              description={t('settings.maxImportFilesDescription', {})}
              min={0}
              max={5000000}
              value={form.max_import_files}
              onChange={(v) => setForm({ ...form, max_import_files: toNumber(v, form.max_import_files) })}
              disabled={!canManage}
            />
          </div>

          <Switch
            label={t('settings.allowSavedLogins', {})}
            description={t('settings.allowSavedLoginsDescription', {})}
            checked={form.allow_saved_logins}
            onChange={(e) => setForm({ ...form, allow_saved_logins: e.currentTarget.checked })}
            disabled={!canManage}
          />

          <Switch
            label={t('settings.allowPrivateNetworks', {})}
            description={t('settings.allowPrivateNetworksDescription', {})}
            checked={form.allow_private_networks}
            onChange={(e) => setForm({ ...form, allow_private_networks: e.currentTarget.checked })}
            disabled={!canManage}
          />

          {canManage ? (
            <Group>
              <Button loading={saving} onClick={() => void save()}>
                {t('settings.save', {})}
              </Button>
            </Group>
          ) : null}
        </Stack>
      )}
    </TitleCard>
  );
}

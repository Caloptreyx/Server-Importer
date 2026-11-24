import { faCloudArrowDown, faFingerprint, faFolderOpen, faPlug, faXmark } from '@fortawesome/free-solid-svg-icons';
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { useEffect, useRef, useState } from 'react';
import { httpErrorToHuman } from '@/api/axios.ts';
import ActionIcon from '@/elements/buttons/ActionIcon.tsx';
import Button from '@/elements/buttons/Button.tsx';
import ServerContentContainer from '@/elements/containers/ServerContentContainer.tsx';
import Badge from '@/elements/data-display/Badge.tsx';
import TitleCard from '@/elements/data-display/TitleCard.tsx';
import Alert from '@/elements/feedback/Alert.tsx';
import NumberInput from '@/elements/input/NumberInput.tsx';
import PasswordInput from '@/elements/input/PasswordInput.tsx';
import TextArea from '@/elements/input/TextArea.tsx';
import TextInput from '@/elements/input/TextInput.tsx';
import Group from '@/elements/layout/Group.tsx';
import SegmentedControl from '@/elements/layout/SegmentedControl.tsx';
import Stack from '@/elements/layout/Stack.tsx';
import Text from '@/elements/typography/Text.tsx';
import { useToast } from '@/providers/ToastProvider.tsx';
import { useServerStore } from '@/stores/server.ts';
import {
  type AuthPayload,
  type BrowseResponse,
  browseRemote,
  cancelImport,
  getStatus,
  PACKAGE,
  type PreviewResponse,
  previewRemote,
  startImport,
} from '../api.ts';
import ActiveImportCard from '../components/ActiveImportCard.tsx';
import ImportHistoryCard from '../components/ImportHistoryCard.tsx';
import RemoteBrowserCard from '../components/RemoteBrowserCard.tsx';
import { formatBytes, parseSftpInput } from '../lib/sftp.ts';
import { useExtTranslations } from '../translations.ts';

type AuthMode = 'password' | 'private_key';

export default function ImporterPage() {
  const server = useServerStore((state) => state.server);
  const { t } = useExtTranslations();
  const { addToast } = useToast();
  const queryClient = useQueryClient();

  const statusKey = [PACKAGE, 'status', server.uuid] as const;
  const statusQuery = useQuery({
    queryKey: statusKey,
    queryFn: () => getStatus(server.uuid),
    refetchInterval: (query) => (query.state.data?.active ? 1200 : false),
  });

  const [host, setHost] = useState('');
  const [port, setPort] = useState<number>(22);
  const [username, setUsername] = useState('');
  const [authMode, setAuthMode] = useState<AuthMode>('password');
  const [password, setPassword] = useState('');
  const [privateKey, setPrivateKey] = useState('');
  const [passphrase, setPassphrase] = useState('');
  const [sourcePath, setSourcePath] = useState('/');
  const [destinationPath, setDestinationPath] = useState('/');
  const [pinnedFingerprint, setPinnedFingerprint] = useState<string | null>(null);

  const [browse, setBrowse] = useState<BrowseResponse | null>(null);
  const [preview, setPreview] = useState<PreviewResponse | null>(null);
  const [browsing, setBrowsing] = useState(false);
  const [previewing, setPreviewing] = useState(false);
  const [starting, setStarting] = useState(false);
  const [cancelling, setCancelling] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);

  const wasActive = useRef(false);
  const active = statusQuery.data?.active ?? null;

  useEffect(() => {
    if (wasActive.current && !active) {
      void queryClient.invalidateQueries({ queryKey: statusKey });
    }
    wasActive.current = Boolean(active);
  }, [active, queryClient, server.uuid]);

  const buildAuth = (): AuthPayload =>
    authMode === 'password'
      ? { type: 'password', password }
      : {
          type: 'private_key',
          private_key: privateKey,
          passphrase: passphrase || undefined,
        };

  const canConnect =
    host.trim().length > 0 &&
    username.trim().length > 0 &&
    port >= 1 &&
    port <= 65535 &&
    (authMode === 'password' ? password.length > 0 : privateKey.trim().length > 0);

  const handleHostChange = (value: string) => {
    const parsed = parseSftpInput(value);
    if (parsed) {
      setHost(parsed.host);
      if (parsed.port) setPort(parsed.port);
      if (parsed.username) setUsername(parsed.username);
      if (parsed.path) setSourcePath(parsed.path);
      setPinnedFingerprint(null);
      setBrowse(null);
      setPreview(null);
      return;
    }
    setHost(value);
    setPinnedFingerprint(null);
  };

  const runBrowse = async (targetPath: string, notify: boolean) => {
    setActionError(null);
    setBrowsing(true);
    setPreview(null);
    try {
      const res = await browseRemote(server.uuid, {
        host: host.trim(),
        port,
        username: username.trim(),
        auth: buildAuth(),
        path: targetPath,
        expected_fingerprint: pinnedFingerprint ?? undefined,
      });
      setBrowse(res);
      setSourcePath(res.resolved_path);
      if (res.host_fingerprint) {
        setPinnedFingerprint(res.host_fingerprint);
      }
      if (notify) {
        addToast(t('connection.connectedToast', {}), 'success');
      }
    } catch (error) {
      setActionError(httpErrorToHuman(error));
    } finally {
      setBrowsing(false);
    }
  };

  const runPreview = async () => {
    const targetPath = browse?.resolved_path ?? sourcePath;
    setActionError(null);
    setPreviewing(true);
    try {
      const res = await previewRemote(server.uuid, {
        host: host.trim(),
        port,
        username: username.trim(),
        auth: buildAuth(),
        path: targetPath,
        expected_fingerprint: pinnedFingerprint ?? undefined,
      });
      setPreview(res);
      if (res.host_fingerprint) {
        setPinnedFingerprint(res.host_fingerprint);
      }
    } catch (error) {
      setActionError(httpErrorToHuman(error));
    } finally {
      setPreviewing(false);
    }
  };

  const handleStart = async () => {
    setActionError(null);
    setStarting(true);
    try {
      const res = await startImport(server.uuid, {
        host: host.trim(),
        port,
        username: username.trim(),
        auth: buildAuth(),
        source_path: sourcePath.trim() || '/',
        destination_path: destinationPath.trim() || '/',
        expected_fingerprint: pinnedFingerprint ?? undefined,
      });
      queryClient.setQueryData(statusKey, (prev: typeof statusQuery.data) =>
        prev ? { ...prev, active: res.active } : prev,
      );
      void queryClient.invalidateQueries({ queryKey: statusKey });
      addToast(t('connection.startedToast', {}), 'success');
    } catch (error) {
      setActionError(httpErrorToHuman(error));
    } finally {
      setStarting(false);
    }
  };

  const handleCancel = async () => {
    setCancelling(true);
    try {
      await cancelImport(server.uuid);
      addToast(t('active.cancelledToast', {}), 'success');
      void queryClient.invalidateQueries({ queryKey: statusKey });
    } catch (error) {
      addToast(httpErrorToHuman(error), 'error');
    } finally {
      setCancelling(false);
    }
  };

  const limits = statusQuery.data?.limits;

  return (
    <ServerContentContainer title={t('title', {})} subtitle={t('subtitle', {})}>
      <Stack gap='md'>
        {statusQuery.error ? <Alert color='red'>{httpErrorToHuman(statusQuery.error)}</Alert> : null}

        {active ? <ActiveImportCard active={active} cancelling={cancelling} onCancel={handleCancel} /> : null}

        <TitleCard title={t('connection.title', {})} icon={<FontAwesomeIcon icon={faPlug} />}>
          <Stack gap='md'>
            <Text size='sm' c='dimmed'>
              {t('connection.description', {})}
            </Text>

            {actionError ? <Alert color='red'>{actionError}</Alert> : null}

            <div className='grid grid-cols-1 md:grid-cols-12 gap-3'>
              <div className='md:col-span-6'>
                <TextInput
                  label={t('connection.host', {})}
                  placeholder={t('connection.hostPlaceholder', {})}
                  value={host}
                  onChange={(e) => handleHostChange(e.currentTarget.value)}
                  disabled={starting}
                />
              </div>
              <div className='md:col-span-2'>
                <NumberInput
                  label={t('connection.port', {})}
                  min={1}
                  max={65535}
                  value={port}
                  onChange={(v) => {
                    const parsed = typeof v === 'number' ? v : Number.parseInt(String(v), 10);
                    if (Number.isFinite(parsed)) setPort(parsed);
                  }}
                  disabled={starting}
                />
              </div>
              <div className='md:col-span-4'>
                <TextInput
                  label={t('connection.username', {})}
                  placeholder={t('connection.usernamePlaceholder', {})}
                  value={username}
                  onChange={(e) => setUsername(e.currentTarget.value)}
                  disabled={starting}
                />
              </div>
            </div>

            <Stack gap='xs'>
              <Text size='sm' fw={500}>
                {t('connection.authMethod', {})}
              </Text>
              <div>
                <SegmentedControl
                  value={authMode}
                  onChange={(val) => setAuthMode(val as AuthMode)}
                  data={[
                    { value: 'password', label: t('connection.authPassword', {}) },
                    { value: 'private_key', label: t('connection.authPrivateKey', {}) },
                  ]}
                  disabled={starting}
                />
              </div>
            </Stack>

            {authMode === 'password' ? (
              <PasswordInput
                label={t('connection.password', {})}
                placeholder={t('connection.passwordPlaceholder', {})}
                value={password}
                onChange={(e) => setPassword(e.currentTarget.value)}
                disabled={starting}
              />
            ) : (
              <Stack gap='sm'>
                <TextArea
                  label={t('connection.privateKey', {})}
                  placeholder={t('connection.privateKeyPlaceholder', {})}
                  value={privateKey}
                  onChange={(e) => setPrivateKey(e.currentTarget.value)}
                  minRows={4}
                  autosize
                  maxRows={8}
                  styles={{ input: { fontFamily: 'monospace', fontSize: '0.8rem' } }}
                  disabled={starting}
                />
                <PasswordInput
                  label={t('connection.passphrase', {})}
                  placeholder={t('connection.passphrasePlaceholder', {})}
                  value={passphrase}
                  onChange={(e) => setPassphrase(e.currentTarget.value)}
                  disabled={starting}
                />
              </Stack>
            )}

            <div className='grid grid-cols-1 md:grid-cols-2 gap-3'>
              <TextInput
                label={t('connection.sourcePath', {})}
                placeholder={t('connection.sourcePathPlaceholder', {})}
                value={sourcePath}
                onChange={(e) => {
                  setSourcePath(e.currentTarget.value);
                  setPreview(null);
                }}
                disabled={starting}
              />
              <TextInput
                label={t('connection.destinationPath', {})}
                placeholder={t('connection.destinationPathPlaceholder', {})}
                value={destinationPath}
                onChange={(e) => setDestinationPath(e.currentTarget.value)}
                disabled={starting}
              />
            </div>

            {pinnedFingerprint ? (
              <Group gap='xs'>
                <Badge color='teal' variant='light' leftSection={<FontAwesomeIcon icon={faFingerprint} />}>
                  {t('connection.fingerprintPinned', { fingerprint: pinnedFingerprint })}
                </Badge>
                <ActionIcon
                  size='xs'
                  variant='subtle'
                  color='gray'
                  onClick={() => setPinnedFingerprint(null)}
                  aria-label={t('connection.fingerprintReset', {})}
                >
                  <FontAwesomeIcon icon={faXmark} />
                </ActionIcon>
              </Group>
            ) : null}

            <Group justify='space-between' wrap='wrap' gap='sm'>
              <Group gap='sm'>
                <Button
                  variant='light'
                  leftSection={<FontAwesomeIcon icon={faFolderOpen} />}
                  loading={browsing}
                  disabled={!canConnect || starting}
                  onClick={() => void runBrowse(sourcePath || '/', true)}
                >
                  {t('connection.testAndBrowse', {})}
                </Button>
                <Button
                  leftSection={<FontAwesomeIcon icon={faCloudArrowDown} />}
                  loading={starting}
                  disabled={!canConnect || Boolean(active) || browsing}
                  onClick={() => void handleStart()}
                >
                  {t('connection.startImport', {})}
                </Button>
              </Group>

              {limits ? (
                <Text size='xs' c='dimmed'>
                  {t('connection.limitsNote', {
                    size:
                      limits.max_import_size_mb > 0
                        ? formatBytes(limits.max_import_size_mb * 1024 * 1024)
                        : t('connection.limitsUnlimited', {}),
                    files:
                      limits.max_import_files > 0
                        ? limits.max_import_files.toLocaleString()
                        : t('connection.limitsUnlimited', {}),
                  })}
                </Text>
              ) : null}
            </Group>
          </Stack>
        </TitleCard>

        {browse ? (
          <RemoteBrowserCard
            browse={browse}
            browsing={browsing}
            preview={preview}
            previewing={previewing}
            onNavigate={(nextPath) => void runBrowse(nextPath, false)}
            onRefresh={() => void runBrowse(browse.resolved_path, false)}
            onPreview={() => void runPreview()}
          />
        ) : null}

        <ImportHistoryCard history={statusQuery.data?.history ?? []} />
      </Stack>
    </ServerContentContainer>
  );
}

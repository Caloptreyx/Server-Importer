import {
  faCloudArrowDown,
  faFloppyDisk,
  faFolderOpen,
  faLock,
  faPlug,
  faShieldHalved,
  faXmark,
} from '@fortawesome/free-solid-svg-icons';
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { type FormEvent, useEffect, useRef, useState } from 'react';
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
import Tooltip from '@/elements/overlays/Tooltip.tsx';
import Text from '@/elements/typography/Text.tsx';
import { useToast } from '@/providers/ToastProvider.tsx';
import { useServerStore } from '@/stores/server.ts';
import {
  type AuthMethod,
  type AuthPayload,
  type BrowseResponse,
  browseRemote,
  cancelImport,
  createSavedLogin,
  deleteSavedLogin,
  getStatus,
  type ImportJobRecord,
  PACKAGE,
  type PreviewResponse,
  previewRemote,
  type SavedLogin,
  startImport,
  updateSavedLogin,
} from '../api.ts';
import ActiveImportCard from '../components/ActiveImportCard.tsx';
import ImportHistoryCard from '../components/ImportHistoryCard.tsx';
import RemoteBrowserCard from '../components/RemoteBrowserCard.tsx';
import {
  DeleteLoginModal,
  SavedLoginsBar,
  type SaveLoginDraft,
  SaveLoginModal,
} from '../components/SavedLoginsSection.tsx';
import { formatBytes, parseSftpInput } from '../lib/sftp.ts';
import { useExtTranslations } from '../translations.ts';

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

  const [selectedLoginId, setSelectedLoginId] = useState<string | null>(null);
  const [host, setHost] = useState('');
  const [port, setPort] = useState<number>(22);
  const [username, setUsername] = useState('');
  const [authMode, setAuthMode] = useState<AuthMethod>('password');
  const [password, setPassword] = useState('');
  const [privateKey, setPrivateKey] = useState('');
  const [passphrase, setPassphrase] = useState('');
  const [sourcePath, setSourcePath] = useState('/');
  const [destinationPath, setDestinationPath] = useState('/');
  const [pinnedFingerprint, setPinnedFingerprint] = useState<string | null>(null);

  const [saveDraft, setSaveDraft] = useState<SaveLoginDraft | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<SavedLogin | null>(null);

  const [browse, setBrowse] = useState<BrowseResponse | null>(null);
  const [preview, setPreview] = useState<PreviewResponse | null>(null);
  const [browsing, setBrowsing] = useState(false);
  const [previewing, setPreviewing] = useState(false);
  const [starting, setStarting] = useState(false);
  const [cancelling, setCancelling] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);

  const browserRef = useRef<HTMLDivElement | null>(null);
  const wasActive = useRef(false);
  const active = statusQuery.data?.active ?? null;
  const savedLogins = statusQuery.data?.saved_logins ?? [];
  const allowSavedLogins = statusQuery.data?.allow_saved_logins ?? false;
  const selectedLogin = savedLogins.find((login) => login.id === selectedLoginId) ?? null;

  useEffect(() => {
    if (wasActive.current && !active) {
      void queryClient.invalidateQueries({ queryKey: statusKey });
    }
    wasActive.current = Boolean(active);
  }, [active, queryClient, server.uuid]);

  useEffect(() => {
    if (selectedLoginId && statusQuery.data && !savedLogins.some((login) => login.id === selectedLoginId)) {
      setSelectedLoginId(null);
    }
  }, [selectedLoginId, savedLogins, statusQuery.data]);

  const hasTypedSecret = authMode === 'password' ? password.length > 0 : privateKey.trim().length > 0;
  const usingSavedCredentials = Boolean(
    selectedLogin && allowSavedLogins && authMode === selectedLogin.auth_method && !hasTypedSecret,
  );

  const buildAuth = (): AuthPayload =>
    authMode === 'password'
      ? { type: 'password', password }
      : {
          type: 'private_key',
          private_key: privateKey,
          passphrase: passphrase || undefined,
        };

  const buildConnection = () => ({
    saved_login_id: usingSavedCredentials && selectedLogin ? selectedLogin.id : undefined,
    host: host.trim(),
    port,
    username: username.trim(),
    auth: hasTypedSecret ? buildAuth() : undefined,
    expected_fingerprint: pinnedFingerprint ?? undefined,
  });

  const hasBasicConnection = host.trim().length > 0 && username.trim().length > 0 && port >= 1 && port <= 65535;
  const canConnect = hasBasicConnection && (hasTypedSecret || usingSavedCredentials);
  const canSaveOrUpdate = allowSavedLogins && hasBasicConnection && (selectedLogin ? true : hasTypedSecret);

  const applySavedLogin = (login: SavedLogin) => {
    setSelectedLoginId(login.id);
    setHost(login.host);
    setPort(login.port);
    setUsername(login.username);
    setAuthMode(login.auth_method === 'private_key' ? 'private_key' : 'password');
    setPassword('');
    setPrivateKey('');
    setPassphrase('');
    setSourcePath(login.default_source_path || '/');
    setPinnedFingerprint(login.host_fingerprint);
    setBrowse(null);
    setPreview(null);
    setActionError(null);
  };

  const clearSelectedLogin = () => {
    setSelectedLoginId(null);
    setPinnedFingerprint(null);
    setBrowse(null);
    setPreview(null);
    setActionError(null);
  };

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
    const hadBrowser = Boolean(browse);
    setActionError(null);
    setBrowsing(true);
    setPreview(null);
    try {
      const conn = buildConnection();
      const res = await browseRemote(server.uuid, {
        ...conn,
        path: targetPath,
      });
      setBrowse(res);
      setSourcePath(res.resolved_path);
      if (res.host_fingerprint) {
        setPinnedFingerprint(res.host_fingerprint);
      }
      if (conn.saved_login_id) {
        void queryClient.invalidateQueries({ queryKey: statusKey });
      }
      if (notify) {
        addToast(t('connection.connectedToast', {}), 'success');
      }
      if (!hadBrowser) {
        window.requestAnimationFrame(() => {
          browserRef.current?.scrollIntoView({ behavior: 'smooth', block: 'nearest' });
        });
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
      const conn = buildConnection();
      const res = await previewRemote(server.uuid, {
        ...conn,
        path: targetPath,
      });
      setPreview(res);
      if (res.host_fingerprint) {
        setPinnedFingerprint(res.host_fingerprint);
      }
      if (conn.saved_login_id) {
        void queryClient.invalidateQueries({ queryKey: statusKey });
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
        ...buildConnection(),
        source_path: sourcePath.trim() || '/',
        destination_path: destinationPath.trim() || '/',
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

  const handleReuseJob = (job: ImportJobRecord) => {
    setSelectedLoginId(null);
    setHost(job.host);
    setPort(job.port);
    setUsername(job.username);
    setAuthMode(job.auth_method === 'private_key' ? 'private_key' : 'password');
    setSourcePath(job.source_path);
    setDestinationPath(job.destination_path);
    setPinnedFingerprint(job.host_fingerprint);
    setBrowse(null);
    setPreview(null);
    setActionError(null);
    addToast(t('connection.reusedToast', {}), 'success');
    window.scrollTo({ top: 0, behavior: 'smooth' });
  };

  const openSaveFromForm = () => {
    if (selectedLogin) {
      setSaveDraft({
        mode: 'edit',
        loginId: selectedLogin.id,
        name: selectedLogin.name,
        host: host.trim(),
        port,
        username: username.trim(),
        authMode,
        password,
        privateKey,
        passphrase,
        defaultSourcePath: sourcePath.trim() || '/',
        hostFingerprint: pinnedFingerprint,
      });
    } else {
      setSaveDraft({
        mode: 'create',
        name: `${username.trim()}@${host.trim()}`,
        host: host.trim(),
        port,
        username: username.trim(),
        authMode,
        password,
        privateKey,
        passphrase,
        defaultSourcePath: sourcePath.trim() || '/',
        hostFingerprint: pinnedFingerprint,
      });
    }
  };

  const openEditLogin = (login: SavedLogin) => {
    setSaveDraft({
      mode: 'edit',
      loginId: login.id,
      name: login.name,
      host: login.host,
      port: login.port,
      username: login.username,
      authMode: login.auth_method === 'private_key' ? 'private_key' : 'password',
      password: '',
      privateKey: '',
      passphrase: '',
      defaultSourcePath: login.default_source_path || '/',
      hostFingerprint: login.host_fingerprint,
    });
  };

  const handleSaveModalSubmit = async (values: {
    mode: 'create' | 'edit';
    loginId?: string;
    name: string;
    host: string;
    port: number;
    username: string;
    auth?: AuthPayload;
    default_source_path: string;
    host_fingerprint?: string;
  }) => {
    if (values.mode === 'create') {
      if (!values.auth) return;
      const created = await createSavedLogin(server.uuid, {
        name: values.name,
        host: values.host,
        port: values.port,
        username: values.username,
        auth: values.auth,
        default_source_path: values.default_source_path,
        host_fingerprint: values.host_fingerprint,
      });
      await queryClient.invalidateQueries({ queryKey: statusKey });
      applySavedLogin(created);
      addToast(t('savedLogins.savedToast', { name: created.name }), 'success');
    } else if (values.loginId) {
      const updated = await updateSavedLogin(server.uuid, values.loginId, {
        name: values.name,
        host: values.host,
        port: values.port,
        username: values.username,
        auth: values.auth,
        default_source_path: values.default_source_path,
        host_fingerprint: values.host_fingerprint,
      });
      await queryClient.invalidateQueries({ queryKey: statusKey });
      if (selectedLoginId === updated.id) {
        applySavedLogin(updated);
      }
      addToast(t('savedLogins.updatedToast', { name: updated.name }), 'success');
    }
  };

  const handleDeleteLogin = async (login: SavedLogin) => {
    await deleteSavedLogin(server.uuid, login.id);
    if (selectedLoginId === login.id) {
      clearSelectedLogin();
    }
    await queryClient.invalidateQueries({ queryKey: statusKey });
    addToast(t('savedLogins.deletedToast', { name: login.name }), 'success');
  };

  const handleFormSubmit = (e: FormEvent) => {
    e.preventDefault();
    if (canConnect && !browsing && !starting) {
      void runBrowse(sourcePath || '/', true);
    }
  };

  const limits = statusQuery.data?.limits;

  return (
    <ServerContentContainer title={t('title', {})} subtitle={t('subtitle', {})}>
      <Stack gap='md'>
        {statusQuery.error ? <Alert color='red'>{httpErrorToHuman(statusQuery.error)}</Alert> : null}

        {active ? <ActiveImportCard active={active} cancelling={cancelling} onCancel={handleCancel} /> : null}

        <TitleCard title={t('connection.title', {})} icon={<FontAwesomeIcon icon={faPlug} />}>
          <form onSubmit={handleFormSubmit}>
            <Stack gap='md'>
              <Text size='sm' c='dimmed'>
                {t('connection.description', {})}
              </Text>

              {allowSavedLogins && savedLogins.length > 0 ? (
                <SavedLoginsBar
                  logins={savedLogins}
                  selectedId={selectedLogin?.id ?? null}
                  disabled={starting || browsing}
                  onSelect={applySavedLogin}
                  onClear={clearSelectedLogin}
                  onEdit={openEditLogin}
                  onDelete={setDeleteTarget}
                />
              ) : null}

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
                    onChange={(val) => setAuthMode(val as AuthMethod)}
                    data={[
                      { value: 'password', label: t('connection.authPassword', {}) },
                      { value: 'private_key', label: t('connection.authPrivateKey', {}) },
                    ]}
                    disabled={starting}
                  />
                </div>
                {usingSavedCredentials && selectedLogin ? (
                  <Alert color='blue' icon={<FontAwesomeIcon icon={faLock} />}>
                    {t('savedLogins.selectedBanner', { name: selectedLogin.name })}
                  </Alert>
                ) : null}
              </Stack>

              {authMode === 'password' ? (
                <PasswordInput
                  label={t('connection.password', {})}
                  placeholder={
                    usingSavedCredentials
                      ? t('connection.passwordSavedPlaceholder', {})
                      : t('connection.passwordPlaceholder', {})
                  }
                  value={password}
                  onChange={(e) => setPassword(e.currentTarget.value)}
                  disabled={starting}
                />
              ) : (
                <Stack gap='sm'>
                  <TextArea
                    label={t('connection.privateKey', {})}
                    placeholder={
                      usingSavedCredentials
                        ? t('connection.privateKeySavedPlaceholder', {})
                        : t('connection.privateKeyPlaceholder', {})
                    }
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
                    placeholder={
                      usingSavedCredentials
                        ? t('connection.passphraseSavedPlaceholder', {})
                        : t('connection.passphrasePlaceholder', {})
                    }
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
                  onChange={(e) => setSourcePath(e.currentTarget.value)}
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
                  <Badge color='teal' variant='light' leftSection={<FontAwesomeIcon icon={faShieldHalved} />}>
                    <span className='font-mono text-[11px]'>
                      {t('connection.fingerprintPinned', { fingerprint: pinnedFingerprint })}
                    </span>
                  </Badge>
                  <Tooltip label={t('connection.fingerprintReset', {})}>
                    <ActionIcon
                      size='xs'
                      variant='subtle'
                      color='gray'
                      onClick={() => setPinnedFingerprint(null)}
                      aria-label={t('connection.fingerprintReset', {})}
                    >
                      <FontAwesomeIcon icon={faXmark} />
                    </ActionIcon>
                  </Tooltip>
                </Group>
              ) : null}

              <Group justify='space-between' wrap='wrap' gap='sm'>
                <Group gap='sm' wrap='wrap'>
                  <Button
                    type='submit'
                    variant='light'
                    leftSection={<FontAwesomeIcon icon={faFolderOpen} />}
                    loading={browsing}
                    disabled={!canConnect || starting}
                  >
                    {t('connection.testAndBrowse', {})}
                  </Button>
                  <Button
                    type='button'
                    leftSection={<FontAwesomeIcon icon={faCloudArrowDown} />}
                    loading={starting}
                    disabled={!canConnect || Boolean(active) || browsing}
                    onClick={() => void handleStart()}
                  >
                    {t('connection.startImport', {})}
                  </Button>
                  {allowSavedLogins ? (
                    <Button
                      type='button'
                      variant='default'
                      leftSection={<FontAwesomeIcon icon={faFloppyDisk} />}
                      disabled={!canSaveOrUpdate || starting}
                      onClick={openSaveFromForm}
                    >
                      {selectedLogin ? t('connection.updateLogin', {}) : t('connection.saveLogin', {})}
                    </Button>
                  ) : null}
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
          </form>
        </TitleCard>

        {browse ? (
          <div ref={browserRef}>
            <RemoteBrowserCard
              browse={browse}
              browsing={browsing}
              preview={preview}
              previewing={previewing}
              canStart={canConnect && !active}
              starting={starting}
              onNavigate={(nextPath) => void runBrowse(nextPath, false)}
              onRefresh={() => void runBrowse(browse.resolved_path, false)}
              onPreview={() => void runPreview()}
              onStartImport={() => void handleStart()}
            />
          </div>
        ) : null}

        <ImportHistoryCard history={statusQuery.data?.history ?? []} onReuse={handleReuseJob} />
      </Stack>

      <SaveLoginModal draft={saveDraft} onClose={() => setSaveDraft(null)} onSubmit={handleSaveModalSubmit} />

      <DeleteLoginModal login={deleteTarget} onClose={() => setDeleteTarget(null)} onConfirm={handleDeleteLogin} />
    </ServerContentContainer>
  );
}

import { faBookmark, faKey, faLock, faPen, faShieldHalved, faTrash, faXmark } from '@fortawesome/free-solid-svg-icons';
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome';
import { type FormEvent, useEffect, useState } from 'react';
import { httpErrorToHuman } from '@/api/axios.ts';
import ActionIcon from '@/elements/buttons/ActionIcon.tsx';
import Button from '@/elements/buttons/Button.tsx';
import Badge from '@/elements/data-display/Badge.tsx';
import Alert from '@/elements/feedback/Alert.tsx';
import NumberInput from '@/elements/input/NumberInput.tsx';
import PasswordInput from '@/elements/input/PasswordInput.tsx';
import TextArea from '@/elements/input/TextArea.tsx';
import TextInput from '@/elements/input/TextInput.tsx';
import Group from '@/elements/layout/Group.tsx';
import SegmentedControl from '@/elements/layout/SegmentedControl.tsx';
import Stack from '@/elements/layout/Stack.tsx';
import ConfirmationModal from '@/elements/modals/ConfirmationModal.tsx';
import { Modal, ModalFooter } from '@/elements/modals/Modal.tsx';
import Tooltip from '@/elements/overlays/Tooltip.tsx';
import Text from '@/elements/typography/Text.tsx';
import type { AuthMethod, AuthPayload, SavedLogin } from '../api.ts';
import { useExtTranslations } from '../translations.ts';

interface SavedLoginsBarProps {
  logins: SavedLogin[];
  selectedId: string | null;
  disabled: boolean;
  onSelect: (login: SavedLogin) => void;
  onClear: () => void;
  onEdit: (login: SavedLogin) => void;
  onDelete: (login: SavedLogin) => void;
}

export function SavedLoginsBar({
  logins,
  selectedId,
  disabled,
  onSelect,
  onClear,
  onEdit,
  onDelete,
}: SavedLoginsBarProps) {
  const { t } = useExtTranslations();

  if (logins.length === 0) {
    return null;
  }

  return (
    <Stack gap='xs'>
      <Group justify='space-between' gap='xs' wrap='wrap'>
        <Group gap='xs'>
          <FontAwesomeIcon icon={faBookmark} className='text-xs text-gray-400' />
          <Text size='sm' fw={600}>
            {t('savedLogins.title', {})}
          </Text>
          <Badge size='xs' variant='light' color='gray'>
            {logins.length}
          </Badge>
        </Group>
        <Group gap='sm'>
          <Text size='xs' c='dimmed'>
            {t('savedLogins.hint', {})}
          </Text>
          {selectedId ? (
            <Button
              type='button'
              size='compact-xs'
              variant='subtle'
              color='gray'
              leftSection={<FontAwesomeIcon icon={faXmark} />}
              onClick={onClear}
              disabled={disabled}
            >
              {t('savedLogins.clearSelection', {})}
            </Button>
          ) : null}
        </Group>
      </Group>

      <div className='grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-2'>
        {logins.map((login) => {
          const isSelected = login.id === selectedId;
          return (
            <div
              key={login.id}
              role='button'
              tabIndex={disabled ? -1 : 0}
              aria-pressed={isSelected}
              onClick={() => {
                if (disabled) return;
                if (isSelected) {
                  onClear();
                } else {
                  onSelect(login);
                }
              }}
              onKeyDown={(e) => {
                if (disabled) return;
                if (e.key === 'Enter' || e.key === ' ') {
                  e.preventDefault();
                  if (isSelected) {
                    onClear();
                  } else {
                    onSelect(login);
                  }
                }
              }}
              className={`group rounded-md border px-3 py-2 transition-colors cursor-pointer select-none ${
                isSelected
                  ? 'border-blue-500/70 bg-blue-500/10'
                  : 'border-white/10 bg-white/[0.02] hover:border-white/20 hover:bg-white/[0.05]'
              } ${disabled ? 'opacity-60 pointer-events-none' : ''}`}
            >
              <Group justify='space-between' align='flex-start' wrap='nowrap' gap='xs'>
                <div className='min-w-0 flex-1'>
                  <Group gap={6} wrap='nowrap'>
                    <FontAwesomeIcon
                      icon={login.auth_method === 'private_key' ? faKey : faLock}
                      className={`text-xs shrink-0 ${isSelected ? 'text-blue-400' : 'text-gray-400'}`}
                    />
                    <Text size='sm' fw={600} truncate>
                      {login.name}
                    </Text>
                    {isSelected ? (
                      <Badge size='xs' variant='filled' color='blue' className='shrink-0'>
                        {t('savedLogins.activeBadge', {})}
                      </Badge>
                    ) : null}
                  </Group>
                  <Text size='xs' c='dimmed' ff='monospace' truncate mt={2}>
                    {login.username}@{login.host}:{login.port}
                  </Text>
                </div>

                <Group gap={2} wrap='nowrap' className='shrink-0'>
                  <Tooltip label={t('savedLogins.edit', {})}>
                    <ActionIcon
                      size='sm'
                      variant='subtle'
                      color='gray'
                      aria-label={t('savedLogins.edit', {})}
                      onClick={(e) => {
                        e.stopPropagation();
                        onEdit(login);
                      }}
                    >
                      <FontAwesomeIcon icon={faPen} className='text-[11px]' />
                    </ActionIcon>
                  </Tooltip>
                  <Tooltip label={t('savedLogins.delete', {})}>
                    <ActionIcon
                      size='sm'
                      variant='subtle'
                      color='red'
                      aria-label={t('savedLogins.delete', {})}
                      onClick={(e) => {
                        e.stopPropagation();
                        onDelete(login);
                      }}
                    >
                      <FontAwesomeIcon icon={faTrash} className='text-[11px]' />
                    </ActionIcon>
                  </Tooltip>
                </Group>
              </Group>
            </div>
          );
        })}
      </div>
    </Stack>
  );
}

export interface SaveLoginDraft {
  mode: 'create' | 'edit';
  loginId?: string;
  name: string;
  host: string;
  port: number;
  username: string;
  authMode: AuthMethod;
  password: string;
  privateKey: string;
  passphrase: string;
  defaultSourcePath: string;
  hostFingerprint: string | null;
}

interface SaveLoginModalProps {
  draft: SaveLoginDraft | null;
  onClose: () => void;
  onSubmit: (values: {
    mode: 'create' | 'edit';
    loginId?: string;
    name: string;
    host: string;
    port: number;
    username: string;
    auth?: AuthPayload;
    default_source_path: string;
    host_fingerprint?: string;
  }) => Promise<void>;
}

export function SaveLoginModal({ draft, onClose, onSubmit }: SaveLoginModalProps) {
  const { t } = useExtTranslations();

  const [name, setName] = useState('');
  const [host, setHost] = useState('');
  const [port, setPort] = useState(22);
  const [username, setUsername] = useState('');
  const [authMode, setAuthMode] = useState<AuthMethod>('password');
  const [password, setPassword] = useState('');
  const [privateKey, setPrivateKey] = useState('');
  const [passphrase, setPassphrase] = useState('');
  const [defaultSourcePath, setDefaultSourcePath] = useState('/');
  const [hostFingerprint, setHostFingerprint] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!draft) return;
    setName(draft.name);
    setHost(draft.host);
    setPort(draft.port);
    setUsername(draft.username);
    setAuthMode(draft.authMode);
    setPassword(draft.password);
    setPrivateKey(draft.privateKey);
    setPassphrase(draft.passphrase);
    setDefaultSourcePath(draft.defaultSourcePath || '/');
    setHostFingerprint(draft.hostFingerprint);
    setError(null);
    setSaving(false);
  }, [draft]);

  const isEdit = draft?.mode === 'edit';
  const authMethodChanged = Boolean(isEdit && draft && authMode !== draft.authMode);
  const hasNewSecret = authMode === 'password' ? password.length > 0 : privateKey.trim().length > 0;
  const canSubmit =
    name.trim().length > 0 &&
    host.trim().length > 0 &&
    username.trim().length > 0 &&
    port >= 1 &&
    port <= 65535 &&
    (isEdit && !authMethodChanged ? true : hasNewSecret);

  const handleSubmit = async (e: FormEvent) => {
    e.preventDefault();
    if (!draft || !canSubmit || saving) return;

    let auth: AuthPayload | undefined;
    if (hasNewSecret) {
      auth =
        authMode === 'password'
          ? { type: 'password', password }
          : {
              type: 'private_key',
              private_key: privateKey,
              passphrase: passphrase || undefined,
            };
    }

    setError(null);
    setSaving(true);
    try {
      await onSubmit({
        mode: draft.mode,
        loginId: draft.loginId,
        name: name.trim(),
        host: host.trim(),
        port,
        username: username.trim(),
        auth,
        default_source_path: defaultSourcePath.trim() || '/',
        host_fingerprint: hostFingerprint ?? undefined,
      });
      onClose();
    } catch (err) {
      setError(httpErrorToHuman(err));
    } finally {
      setSaving(false);
    }
  };

  return (
    <Modal
      opened={Boolean(draft)}
      onClose={onClose}
      title={isEdit ? t('savedLogins.editModalTitle', {}) : t('savedLogins.saveModalTitle', {})}
      size='lg'
    >
      <form onSubmit={(e) => void handleSubmit(e)}>
        <Stack gap='sm'>
          <Text size='sm' c='dimmed'>
            {t('savedLogins.saveDescription', {})}
          </Text>

          {error ? <Alert color='red'>{error}</Alert> : null}

          <TextInput
            label={t('savedLogins.nameLabel', {})}
            placeholder={t('savedLogins.namePlaceholder', {})}
            value={name}
            onChange={(e) => setName(e.currentTarget.value)}
            maxLength={128}
            required
            autoFocus
            disabled={saving}
          />

          <div className='grid grid-cols-1 md:grid-cols-12 gap-3'>
            <div className='md:col-span-6'>
              <TextInput
                label={t('connection.host', {})}
                value={host}
                onChange={(e) => {
                  setHost(e.currentTarget.value);
                  setHostFingerprint(null);
                }}
                required
                disabled={saving}
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
                disabled={saving}
              />
            </div>
            <div className='md:col-span-4'>
              <TextInput
                label={t('connection.username', {})}
                value={username}
                onChange={(e) => setUsername(e.currentTarget.value)}
                required
                disabled={saving}
              />
            </div>
          </div>

          <TextInput
            label={t('connection.sourcePath', {})}
            placeholder={t('connection.sourcePathPlaceholder', {})}
            value={defaultSourcePath}
            onChange={(e) => setDefaultSourcePath(e.currentTarget.value)}
            disabled={saving}
          />

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
                disabled={saving}
              />
            </div>
            {isEdit ? (
              <Text size='xs' c='dimmed'>
                {t('savedLogins.editCredentialsHint', {})}
              </Text>
            ) : null}
          </Stack>

          {authMode === 'password' ? (
            <PasswordInput
              label={t('connection.password', {})}
              placeholder={
                isEdit ? t('connection.passwordSavedPlaceholder', {}) : t('connection.passwordPlaceholder', {})
              }
              value={password}
              onChange={(e) => setPassword(e.currentTarget.value)}
              disabled={saving}
            />
          ) : (
            <Stack gap='xs'>
              <TextArea
                label={t('connection.privateKey', {})}
                placeholder={
                  isEdit ? t('connection.privateKeySavedPlaceholder', {}) : t('connection.privateKeyPlaceholder', {})
                }
                value={privateKey}
                onChange={(e) => setPrivateKey(e.currentTarget.value)}
                minRows={3}
                autosize
                maxRows={6}
                styles={{ input: { fontFamily: 'monospace', fontSize: '0.8rem' } }}
                disabled={saving}
              />
              <PasswordInput
                label={t('connection.passphrase', {})}
                placeholder={
                  isEdit ? t('connection.passphraseSavedPlaceholder', {}) : t('connection.passphrasePlaceholder', {})
                }
                value={passphrase}
                onChange={(e) => setPassphrase(e.currentTarget.value)}
                disabled={saving}
              />
            </Stack>
          )}

          {hostFingerprint ? (
            <Group gap='xs'>
              <Badge color='teal' variant='light' leftSection={<FontAwesomeIcon icon={faShieldHalved} />}>
                <span className='font-mono text-[11px]'>
                  {t('connection.fingerprintPinned', { fingerprint: hostFingerprint })}
                </span>
              </Badge>
              <Tooltip label={t('connection.fingerprintReset', {})}>
                <ActionIcon
                  size='xs'
                  variant='subtle'
                  color='gray'
                  onClick={() => setHostFingerprint(null)}
                  aria-label={t('connection.fingerprintReset', {})}
                >
                  <FontAwesomeIcon icon={faXmark} />
                </ActionIcon>
              </Tooltip>
            </Group>
          ) : null}
        </Stack>

        <ModalFooter>
          <Button type='submit' loading={saving} disabled={!canSubmit}>
            {isEdit ? t('savedLogins.updateButton', {}) : t('savedLogins.saveButton', {})}
          </Button>
          <Button type='button' variant='default' onClick={onClose} disabled={saving}>
            {t('savedLogins.cancelButton', {})}
          </Button>
        </ModalFooter>
      </form>
    </Modal>
  );
}

interface DeleteLoginModalProps {
  login: SavedLogin | null;
  onClose: () => void;
  onConfirm: (login: SavedLogin) => Promise<void>;
}

export function DeleteLoginModal({ login, onClose, onConfirm }: DeleteLoginModalProps) {
  const { t } = useExtTranslations();

  return (
    <ConfirmationModal
      opened={Boolean(login)}
      onClose={onClose}
      title={t('savedLogins.deleteModalTitle', {})}
      confirm={t('savedLogins.deleteConfirm', {})}
      confirmColor='red'
      onConfirmed={async () => {
        if (!login) return;
        await onConfirm(login);
        onClose();
      }}
    >
      {login ? (
        <Text size='sm'>
          {t('savedLogins.deleteModalBody', {
            name: login.name,
            target: `${login.username}@${login.host}:${login.port}`,
          })}
        </Text>
      ) : null}
    </ConfirmationModal>
  );
}

import {
  faArrowRight,
  faBan,
  faCloudArrowDown,
  faFingerprint,
  faTriangleExclamation,
} from '@fortawesome/free-solid-svg-icons';
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome';
import Button from '@/elements/buttons/Button.tsx';
import Badge from '@/elements/data-display/Badge.tsx';
import TitleCard from '@/elements/data-display/TitleCard.tsx';
import Alert from '@/elements/feedback/Alert.tsx';
import Progress from '@/elements/feedback/Progress.tsx';
import Group from '@/elements/layout/Group.tsx';
import Stack from '@/elements/layout/Stack.tsx';
import Code from '@/elements/typography/Code.tsx';
import Text from '@/elements/typography/Text.tsx';
import type { ActiveImport, ImportPhase } from '../api.ts';
import { formatBytes, formatDuration, formatSpeed, progressPercent } from '../lib/sftp.ts';
import { useExtTranslations } from '../translations.ts';

interface Props {
  active: ActiveImport;
  cancelling: boolean;
  onCancel: () => void;
}

export default function ActiveImportCard({ active, cancelling, onCancel }: Props) {
  const { t } = useExtTranslations();

  const phaseLabel = (phase: ImportPhase) => {
    switch (phase) {
      case 'connecting':
        return t('active.phaseConnecting', {});
      case 'scanning':
        return t('active.phaseScanning', {});
      case 'transferring':
        return t('active.phaseTransferring', {});
    }
  };

  const indeterminate = active.phase !== 'transferring';
  const pct = progressPercent(
    active.bytes_transferred,
    active.bytes_total,
    active.files_transferred,
    active.files_total,
  );

  return (
    <TitleCard
      title={t('active.title', {})}
      icon={<FontAwesomeIcon icon={faCloudArrowDown} />}
      rightSection={
        <Group gap='xs'>
          <Badge color='blue' variant='light'>
            {phaseLabel(active.phase)}
          </Badge>
          <Button
            color='red'
            variant='light'
            size='xs'
            leftSection={<FontAwesomeIcon icon={faBan} />}
            loading={cancelling}
            onClick={onCancel}
          >
            {t('active.cancel', {})}
          </Button>
        </Group>
      }
    >
      <Stack gap='sm'>
        <Group gap='xs' wrap='wrap'>
          <Code>
            {active.username}@{active.host}:{active.port}
          </Code>
          <Code>{active.source_path}</Code>
          <FontAwesomeIcon icon={faArrowRight} className='text-xs opacity-60' />
          <Code>{active.destination_path}</Code>
        </Group>

        <Progress value={pct} indeterminate={indeterminate} color='blue' />

        <div className='grid grid-cols-2 sm:grid-cols-4 gap-2 text-sm'>
          <div>
            <Text size='xs' c='dimmed'>
              {t('active.filesProgress', {
                transferred: active.files_transferred.toLocaleString(),
                total: active.files_total.toLocaleString(),
              })}
            </Text>
          </div>
          <div>
            <Text size='xs' c='dimmed'>
              {t('active.bytesProgress', {
                transferred: formatBytes(active.bytes_transferred),
                total: formatBytes(active.bytes_total),
              })}
            </Text>
          </div>
          <div>
            <Text size='xs' c='dimmed'>
              {t('active.speed', {})}: {formatSpeed(active.bytes_per_second)}
            </Text>
          </div>
          <div>
            <Text size='xs' c='dimmed'>
              {t('active.eta', {})}: {active.eta_seconds !== null ? formatDuration(active.eta_seconds) : '—'}
            </Text>
          </div>
        </div>

        <Group gap='xs' wrap='wrap'>
          {active.host_fingerprint ? (
            <Badge color='gray' variant='light' leftSection={<FontAwesomeIcon icon={faFingerprint} />}>
              {active.host_fingerprint}
            </Badge>
          ) : null}
          {active.files_skipped > 0 ? (
            <Badge color='gray' variant='light'>
              {t('active.skippedCount', { count: active.files_skipped.toLocaleString() })}
            </Badge>
          ) : null}
          {active.files_failed > 0 ? (
            <Badge color='orange' variant='light'>
              {t('active.failedCount', { count: active.files_failed.toLocaleString() })}
            </Badge>
          ) : null}
        </Group>

        {active.current_file ? (
          <Text size='xs' c='dimmed' className='truncate'>
            {t('active.currentFile', {})}: <Code>{active.current_file}</Code>
          </Text>
        ) : null}

        {active.recent_errors.length > 0 ? (
          <Alert
            color='orange'
            icon={<FontAwesomeIcon icon={faTriangleExclamation} />}
            title={t('active.recentErrors', {})}
          >
            <Stack gap={4}>
              {active.recent_errors.map((err, idx) => (
                <Text key={idx} size='xs' className='break-all'>
                  {err}
                </Text>
              ))}
            </Stack>
          </Alert>
        ) : null}
      </Stack>
    </TitleCard>
  );
}

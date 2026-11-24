import {
  faArrowRight,
  faBan,
  faCheck,
  faClockRotateLeft,
  faSpinner,
  faTriangleExclamation,
} from '@fortawesome/free-solid-svg-icons';
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome';
import Badge from '@/elements/data-display/Badge.tsx';
import Card from '@/elements/data-display/Card.tsx';
import TitleCard from '@/elements/data-display/TitleCard.tsx';
import Group from '@/elements/layout/Group.tsx';
import Stack from '@/elements/layout/Stack.tsx';
import FormattedTimestamp from '@/elements/time/FormattedTimestamp.tsx';
import Code from '@/elements/typography/Code.tsx';
import Text from '@/elements/typography/Text.tsx';
import type { ImportJobRecord, JobStatus } from '../api.ts';
import { formatBytes, formatDuration } from '../lib/sftp.ts';
import { useExtTranslations } from '../translations.ts';

interface Props {
  history: ImportJobRecord[];
}

const STATUS_COLOR: Record<JobStatus, string> = {
  running: 'blue',
  completed: 'green',
  failed: 'red',
  cancelled: 'gray',
};

const STATUS_STRIPE: Record<JobStatus, string> = {
  running: 'bg-blue-500',
  completed: 'bg-green-500',
  failed: 'bg-red-500',
  cancelled: 'bg-gray-500',
};

const STATUS_ICON: Record<JobStatus, typeof faCheck> = {
  running: faSpinner,
  completed: faCheck,
  failed: faTriangleExclamation,
  cancelled: faBan,
};

export default function ImportHistoryCard({ history }: Props) {
  const { t } = useExtTranslations();

  const statusLabel = (status: JobStatus) => {
    switch (status) {
      case 'running':
        return t('history.statusRunning', {});
      case 'completed':
        return t('history.statusCompleted', {});
      case 'failed':
        return t('history.statusFailed', {});
      case 'cancelled':
        return t('history.statusCancelled', {});
    }
  };

  return (
    <TitleCard title={t('history.title', {})} icon={<FontAwesomeIcon icon={faClockRotateLeft} />}>
      {history.length === 0 ? (
        <Text size='sm' c='dimmed'>
          {t('history.empty', {})}
        </Text>
      ) : (
        <Stack gap='sm'>
          {history.map((job) => {
            const startedMs = Date.parse(job.started_at);
            const finishedMs = job.finished_at ? Date.parse(job.finished_at) : Number.NaN;
            const durationSecs =
              Number.isFinite(startedMs) && Number.isFinite(finishedMs)
                ? Math.max(0, Math.floor((finishedMs - startedMs) / 1000))
                : null;

            return (
              <Card key={job.id} p='sm' leftStripeClassName={STATUS_STRIPE[job.status]}>
                <Stack gap='xs'>
                  <div className='flex flex-col sm:flex-row sm:items-center justify-between gap-2'>
                    <Group gap='xs' wrap='wrap'>
                      <Badge
                        color={STATUS_COLOR[job.status]}
                        variant='light'
                        leftSection={<FontAwesomeIcon icon={STATUS_ICON[job.status]} spin={job.status === 'running'} />}
                      >
                        {statusLabel(job.status)}
                      </Badge>
                      <Code>
                        {job.username}@{job.host}:{job.port}
                      </Code>
                      <Code>{job.source_path}</Code>
                      <FontAwesomeIcon icon={faArrowRight} className='text-xs opacity-60' />
                      <Code>{job.destination_path}</Code>
                    </Group>

                    <Text size='xs' c='dimmed' className='shrink-0'>
                      <FormattedTimestamp timestamp={job.started_at} />
                    </Text>
                  </div>

                  <Group gap='md' wrap='wrap'>
                    <Text size='xs' c='dimmed'>
                      {t('history.filesSummary', {
                        transferred: job.files_transferred.toLocaleString(),
                        total: job.files_total.toLocaleString(),
                      })}
                    </Text>
                    <Text size='xs' c='dimmed'>
                      {formatBytes(job.bytes_transferred)}
                      {job.bytes_total > 0 ? ` / ${formatBytes(job.bytes_total)}` : ''}
                    </Text>
                    {job.files_skipped > 0 ? (
                      <Text size='xs' c='dimmed'>
                        {t('history.skippedSummary', { count: job.files_skipped.toLocaleString() })}
                      </Text>
                    ) : null}
                    {job.files_failed > 0 ? (
                      <Text size='xs' c='orange'>
                        {t('history.failedSummary', { count: job.files_failed.toLocaleString() })}
                      </Text>
                    ) : null}
                    {durationSecs !== null ? (
                      <Text size='xs' c='dimmed'>
                        {t('history.duration', { duration: formatDuration(durationSecs) })}
                      </Text>
                    ) : null}
                    {job.user_username ? (
                      <Text size='xs' c='dimmed'>
                        {t('history.byUser', { username: job.user_username })}
                      </Text>
                    ) : null}
                  </Group>

                  {job.error ? (
                    <Text size='xs' c='red' className='break-all'>
                      {job.error}
                    </Text>
                  ) : null}
                </Stack>
              </Card>
            );
          })}
        </Stack>
      )}
    </TitleCard>
  );
}

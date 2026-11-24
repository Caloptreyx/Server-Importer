import {
  faArrowUp,
  faCalculator,
  faFile,
  faFingerprint,
  faFolder,
  faFolderOpen,
  faLink,
  faRotateRight,
  faTriangleExclamation,
} from '@fortawesome/free-solid-svg-icons';
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome';
import ActionIcon from '@/elements/buttons/ActionIcon.tsx';
import Button from '@/elements/buttons/Button.tsx';
import Badge from '@/elements/data-display/Badge.tsx';
import TitleCard from '@/elements/data-display/TitleCard.tsx';
import Alert from '@/elements/feedback/Alert.tsx';
import Spinner from '@/elements/feedback/Spinner.tsx';
import Group from '@/elements/layout/Group.tsx';
import ScrollArea from '@/elements/layout/ScrollArea.tsx';
import Stack from '@/elements/layout/Stack.tsx';
import Tooltip from '@/elements/overlays/Tooltip.tsx';
import FormattedTimestamp from '@/elements/time/FormattedTimestamp.tsx';
import Code from '@/elements/typography/Code.tsx';
import Text from '@/elements/typography/Text.tsx';
import type { BrowseResponse, PreviewResponse, RemoteEntry } from '../api.ts';
import { formatBytes, parentPath, pathSegments } from '../lib/sftp.ts';
import { useExtTranslations } from '../translations.ts';

interface Props {
  browse: BrowseResponse;
  browsing: boolean;
  preview: PreviewResponse | null;
  previewing: boolean;
  onNavigate: (path: string) => void;
  onRefresh: () => void;
  onPreview: () => void;
}

export default function RemoteBrowserCard({
  browse,
  browsing,
  preview,
  previewing,
  onNavigate,
  onRefresh,
  onPreview,
}: Props) {
  const { t } = useExtTranslations();
  const segments = pathSegments(browse.resolved_path);
  const atRoot = browse.resolved_path === '/' || browse.resolved_path === '';

  const entryIcon = (entry: RemoteEntry) => {
    switch (entry.kind) {
      case 'directory':
        return <FontAwesomeIcon icon={faFolder} className='text-blue-400' />;
      case 'symlink':
        return <FontAwesomeIcon icon={faLink} className='text-yellow-400' />;
      default:
        return <FontAwesomeIcon icon={faFile} className='opacity-70' />;
    }
  };

  return (
    <TitleCard
      title={t('browser.title', {})}
      icon={<FontAwesomeIcon icon={faFolderOpen} />}
      rightSection={
        <Group gap='xs'>
          <Badge color='gray' variant='light'>
            {t('browser.itemsCount', { count: browse.entries.length.toLocaleString() })}
          </Badge>
          <Button
            size='xs'
            variant='light'
            leftSection={<FontAwesomeIcon icon={faCalculator} />}
            loading={previewing}
            disabled={browsing}
            onClick={onPreview}
          >
            {t('browser.scanSize', {})}
          </Button>
          <Tooltip label={t('browser.refresh', {})}>
            <ActionIcon variant='subtle' onClick={onRefresh} disabled={browsing}>
              <FontAwesomeIcon icon={faRotateRight} />
            </ActionIcon>
          </Tooltip>
        </Group>
      }
    >
      <Stack gap='sm'>
        <Group justify='space-between' gap='xs' wrap='wrap'>
          <Group gap={4} wrap='wrap'>
            <Tooltip label={t('browser.up', {})}>
              <ActionIcon
                variant='subtle'
                size='sm'
                disabled={atRoot || browsing}
                onClick={() => onNavigate(parentPath(browse.resolved_path))}
              >
                <FontAwesomeIcon icon={faArrowUp} />
              </ActionIcon>
            </Tooltip>
            {segments.map((seg, idx) => (
              <Group key={seg.path} gap={4}>
                {idx > 0 ? (
                  <Text size='xs' c='dimmed'>
                    /
                  </Text>
                ) : null}
                <Button
                  size='compact-xs'
                  variant={idx === segments.length - 1 ? 'light' : 'subtle'}
                  disabled={browsing}
                  onClick={() => onNavigate(seg.path)}
                >
                  {seg.label}
                </Button>
              </Group>
            ))}
          </Group>

          {browse.host_fingerprint ? (
            <Tooltip label={t('browser.hostKey', {})}>
              <Badge color='teal' variant='light' leftSection={<FontAwesomeIcon icon={faFingerprint} />}>
                {browse.host_fingerprint}
              </Badge>
            </Tooltip>
          ) : null}
        </Group>

        <Text size='xs' c='dimmed'>
          {t('browser.useFolder', { path: browse.resolved_path })}
        </Text>

        {preview ? (
          <Alert
            color={preview.truncated ? 'orange' : 'blue'}
            icon={preview.truncated ? <FontAwesomeIcon icon={faTriangleExclamation} /> : undefined}
          >
            <Stack gap={2}>
              <Text size='sm'>
                {t('browser.previewSummary', {
                  files: preview.file_count.toLocaleString(),
                  dirs: preview.directory_count.toLocaleString(),
                  size: formatBytes(preview.total_bytes),
                })}
              </Text>
              {preview.symlink_count > 0 ? (
                <Text size='xs' c='dimmed'>
                  {t('browser.previewSymlinks', { count: preview.symlink_count.toLocaleString() })}
                </Text>
              ) : null}
              {preview.truncated && preview.truncation_reason ? (
                <Text size='xs'>{t('browser.previewTruncated', { reason: preview.truncation_reason })}</Text>
              ) : null}
            </Stack>
          </Alert>
        ) : null}

        {browsing ? (
          <Group justify='center' py='lg'>
            <Spinner />
          </Group>
        ) : browse.entries.length === 0 ? (
          <Text size='sm' c='dimmed' py='sm'>
            {t('browser.empty', {})}
          </Text>
        ) : (
          <ScrollArea mah={320} type='auto' offsetScrollbars>
            <Stack gap={2}>
              {browse.entries.map((entry) => {
                const isDir = entry.kind === 'directory';
                return (
                  <button
                    key={entry.path}
                    type='button'
                    disabled={!isDir}
                    onClick={() => {
                      if (isDir) onNavigate(entry.path);
                    }}
                    className={`w-full flex items-center justify-between gap-3 px-2.5 py-1.5 rounded text-left text-sm transition-colors ${
                      isDir ? 'hover:bg-white/5 cursor-pointer' : 'cursor-default opacity-90'
                    }`}
                  >
                    <Group gap='xs' className='min-w-0 flex-1'>
                      {entryIcon(entry)}
                      <Text size='sm' className='truncate'>
                        {entry.name}
                      </Text>
                      {entry.kind === 'symlink' ? (
                        <Badge size='xs' color='yellow' variant='light'>
                          {t('browser.symlinkSkipped', {})}
                        </Badge>
                      ) : null}
                    </Group>

                    <Group gap='md' className='shrink-0'>
                      <Text size='xs' c='dimmed' className='tabular-nums'>
                        {isDir ? t('browser.folderKind', {}) : formatBytes(entry.size)}
                      </Text>
                      {entry.modified_at ? (
                        <Text size='xs' c='dimmed' className='hidden sm:block'>
                          <FormattedTimestamp timestamp={new Date(entry.modified_at * 1000).toISOString()} />
                        </Text>
                      ) : (
                        <Code className='hidden sm:block opacity-0'>—</Code>
                      )}
                    </Group>
                  </button>
                );
              })}
            </Stack>
          </ScrollArea>
        )}
      </Stack>
    </TitleCard>
  );
}

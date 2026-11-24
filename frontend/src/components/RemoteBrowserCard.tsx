import {
  faArrowUp,
  faCalculator,
  faChevronRight,
  faCloudArrowDown,
  faFile,
  faFolder,
  faFolderOpen,
  faLink,
  faMagnifyingGlass,
  faRotateRight,
  faTriangleExclamation,
  faXmark,
} from '@fortawesome/free-solid-svg-icons';
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome';
import { useEffect, useMemo, useState } from 'react';
import ActionIcon from '@/elements/buttons/ActionIcon.tsx';
import Button from '@/elements/buttons/Button.tsx';
import Badge from '@/elements/data-display/Badge.tsx';
import TitleCard from '@/elements/data-display/TitleCard.tsx';
import Alert from '@/elements/feedback/Alert.tsx';
import Spinner from '@/elements/feedback/Spinner.tsx';
import TextInput from '@/elements/input/TextInput.tsx';
import Group from '@/elements/layout/Group.tsx';
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
  canStart: boolean;
  starting: boolean;
  onNavigate: (path: string) => void;
  onRefresh: () => void;
  onPreview: () => void;
  onStartImport: () => void;
}

export default function RemoteBrowserCard({
  browse,
  browsing,
  preview,
  previewing,
  canStart,
  starting,
  onNavigate,
  onRefresh,
  onPreview,
  onStartImport,
}: Props) {
  const { t } = useExtTranslations();
  const [filter, setFilter] = useState('');

  useEffect(() => {
    setFilter('');
  }, [browse.resolved_path]);

  const segments = pathSegments(browse.resolved_path);
  const atRoot = browse.resolved_path === '/' || browse.resolved_path === '';

  const filteredEntries = useMemo(() => {
    const query = filter.trim().toLowerCase();
    if (!query) return browse.entries;
    return browse.entries.filter((entry) => entry.name.toLowerCase().includes(query));
  }, [browse.entries, filter]);

  const entryIcon = (entry: RemoteEntry) => {
    switch (entry.kind) {
      case 'directory':
        return <FontAwesomeIcon icon={faFolder} className='text-blue-400 shrink-0 w-4' />;
      case 'symlink':
        return <FontAwesomeIcon icon={faLink} className='text-yellow-400 shrink-0 w-4' />;
      default:
        return <FontAwesomeIcon icon={faFile} className='opacity-60 shrink-0 w-4' />;
    }
  };

  return (
    <TitleCard
      title={t('browser.title', {})}
      icon={<FontAwesomeIcon icon={faFolderOpen} />}
      rightSection={
        <Group gap='xs' className='ml-auto'>
          <Badge color='gray' variant='light'>
            {filter.trim()
              ? t('browser.filteredCount', {
                  shown: filteredEntries.length.toLocaleString(),
                  total: browse.entries.length.toLocaleString(),
                })
              : t('browser.itemsCount', { count: browse.entries.length.toLocaleString() })}
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
          <Button
            size='xs'
            leftSection={<FontAwesomeIcon icon={faCloudArrowDown} />}
            loading={starting}
            disabled={!canStart || browsing}
            onClick={onStartImport}
          >
            {t('browser.importFolder', {})}
          </Button>
          <Tooltip label={t('browser.refresh', {})}>
            <ActionIcon variant='subtle' onClick={onRefresh} disabled={browsing} aria-label={t('browser.refresh', {})}>
              <FontAwesomeIcon icon={faRotateRight} spin={browsing} />
            </ActionIcon>
          </Tooltip>
        </Group>
      }
    >
      <Stack gap='sm'>
        <div className='flex flex-col sm:flex-row sm:items-center justify-between gap-2'>
          <div className='flex items-center gap-1 flex-wrap min-w-0'>
            <Tooltip label={t('browser.up', {})}>
              <ActionIcon
                variant='light'
                size='sm'
                disabled={atRoot || browsing}
                onClick={() => onNavigate(parentPath(browse.resolved_path))}
                aria-label={t('browser.up', {})}
              >
                <FontAwesomeIcon icon={faArrowUp} />
              </ActionIcon>
            </Tooltip>
            {segments.map((seg, idx) => (
              <div key={seg.path} className='flex items-center gap-1'>
                {idx > 0 ? <FontAwesomeIcon icon={faChevronRight} className='text-[10px] opacity-40 mx-0.5' /> : null}
                <Button
                  size='compact-xs'
                  variant={idx === segments.length - 1 ? 'light' : 'subtle'}
                  disabled={browsing}
                  onClick={() => onNavigate(seg.path)}
                >
                  {seg.label}
                </Button>
              </div>
            ))}
          </div>

          {browse.entries.length > 6 ? (
            <div className='w-full sm:w-56 shrink-0'>
              <TextInput
                size='xs'
                value={filter}
                onChange={(e) => setFilter(e.currentTarget.value)}
                placeholder={t('browser.filterPlaceholder', {})}
                leftSection={<FontAwesomeIcon icon={faMagnifyingGlass} className='text-xs opacity-60' />}
                rightSection={
                  filter ? (
                    <ActionIcon size='xs' variant='subtle' color='gray' onClick={() => setFilter('')}>
                      <FontAwesomeIcon icon={faXmark} />
                    </ActionIcon>
                  ) : null
                }
              />
            </div>
          ) : null}
        </div>

        <Group gap='xs' wrap='wrap'>
          <Text size='xs' c='dimmed'>
            {t('browser.selectedSource', {})}:
          </Text>
          <Code>{browse.resolved_path}</Code>
        </Group>

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

        <div className='relative rounded-md border border-(--mantine-color-default-border) overflow-hidden'>
          {browsing ? (
            <div className='absolute inset-0 z-10 flex items-center justify-center bg-(--mantine-color-body)/60 backdrop-blur-[1px]'>
              <Spinner />
            </div>
          ) : null}

          {browse.entries.length === 0 ? (
            <div className='px-4 py-8 text-center'>
              <Text size='sm' c='dimmed'>
                {t('browser.empty', {})}
              </Text>
            </div>
          ) : filteredEntries.length === 0 ? (
            <div className='px-4 py-8 text-center'>
              <Text size='sm' c='dimmed'>
                {t('browser.noMatches', { query: filter.trim() })}
              </Text>
            </div>
          ) : (
            <div className='max-h-[400px] overflow-y-auto overscroll-contain'>
              <div className='sticky top-0 z-[1] grid grid-cols-[minmax(0,1fr)_auto] sm:grid-cols-[minmax(0,1fr)_6.5rem_9.5rem] items-center gap-3 px-3 py-2 text-xs font-medium text-(--mantine-color-dimmed) bg-(--mantine-color-default) border-b border-(--mantine-color-default-border)'>
                <span>{t('browser.colName', {})}</span>
                <span className='text-right'>{t('browser.colSize', {})}</span>
                <span className='hidden sm:block text-right'>{t('browser.colModified', {})}</span>
              </div>

              <div className='divide-y divide-(--mantine-color-default-border)/60'>
                {filteredEntries.map((entry) => {
                  const isDir = entry.kind === 'directory';
                  const rowContent = (
                    <>
                      <div className='flex items-center gap-2.5 min-w-0'>
                        {entryIcon(entry)}
                        <Text size='sm' fw={isDir ? 500 : 400} className='truncate'>
                          {entry.name}
                        </Text>
                        {entry.kind === 'symlink' ? (
                          <Badge size='xs' color='yellow' variant='light' className='shrink-0'>
                            {t('browser.symlinkSkipped', {})}
                          </Badge>
                        ) : null}
                      </div>

                      <Text size='xs' c='dimmed' className='text-right tabular-nums shrink-0'>
                        {isDir ? t('browser.folderKind', {}) : formatBytes(entry.size)}
                      </Text>

                      <Text size='xs' c='dimmed' className='hidden sm:block text-right tabular-nums shrink-0'>
                        {entry.modified_at ? (
                          <FormattedTimestamp timestamp={new Date(entry.modified_at * 1000).toISOString()} />
                        ) : (
                          '—'
                        )}
                      </Text>
                    </>
                  );

                  if (isDir) {
                    return (
                      <button
                        key={entry.path}
                        type='button'
                        disabled={browsing}
                        onClick={() => onNavigate(entry.path)}
                        className='w-full grid grid-cols-[minmax(0,1fr)_auto] sm:grid-cols-[minmax(0,1fr)_6.5rem_9.5rem] items-center gap-3 px-3 py-2 text-left transition-colors hover:bg-(--mantine-color-default-hover) cursor-pointer disabled:opacity-60'
                      >
                        {rowContent}
                      </button>
                    );
                  }

                  return (
                    <div
                      key={entry.path}
                      className='w-full grid grid-cols-[minmax(0,1fr)_auto] sm:grid-cols-[minmax(0,1fr)_6.5rem_9.5rem] items-center gap-3 px-3 py-2 text-left opacity-90'
                    >
                      {rowContent}
                    </div>
                  );
                })}
              </div>
            </div>
          )}
        </div>
      </Stack>
    </TitleCard>
  );
}

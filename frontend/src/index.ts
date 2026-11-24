import { faFileImport } from '@fortawesome/free-solid-svg-icons';
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome';
import { createElement } from 'react';
import { Extension, type ExtensionContext } from 'shared';
import ImporterPage from './pages/ImporterPage.tsx';
import SettingsPage from './pages/SettingsPage.tsx';
import { getExtTranslations } from './translations.ts';

class DevCaloptreyxServerImporterExtension extends Extension {
  public cardIcon = createElement(FontAwesomeIcon, { icon: faFileImport });
  public cardConfigurationPage: React.FC | null = SettingsPage;

  public initialize(ctx: ExtensionContext): void {
    ctx.extensionRegistry.enterRoutes((routes) =>
      routes.addServerRoute({
        name: () => getExtTranslations().t('title', {}),
        icon: faFileImport,
        path: '/importer',
        element: ImporterPage,
        permission: 'server-importer.import',
      }),
    );

    ctx.extensionRegistry.enterPermissionIcons((icons) =>
      icons.addServerPermissionIcon('server-importer', createElement(FontAwesomeIcon, { icon: faFileImport })),
    );
  }
}

export default new DevCaloptreyxServerImporterExtension();

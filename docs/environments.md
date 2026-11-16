# Environments

Open the environment selector in the collection toolbar and choose **Manage environments**. Each environment opens in its own tab. Opening an editor does not change the active environment; use **Set active** or select its name in the toolbar.

The sidebar groups **Globals**, **Workspace**, and **Collection** variables. Workspace environments are shared by all collections in the workspace; collection environments apply only to the named collection and override workspace environment variables with the same name. Both levels can be active at once. The collection name is shown below its heading; hover over either group heading for its scope.

Use the `+` beside a group to create an environment and immediately edit its name. The import icon next to `+` imports into that same group. Changes to values are saved automatically. Enter or leaving the name field commits a rename; switching tabs and closing the app also preserve pending names. Ctrl+S saves explicitly.

The toolbar dropdown uses the same font, icons and row spacing as collection context menus. Checkmarks identify active environments in each group. Click a name to activate it, the pencil to open its editor, or the trash icon to open its editor and request deletion. Deletion requires confirmation; closing the dialog leaves the environment intact. Long names are shortened with their full text available on hover; long lists scroll while the management actions stay visible.

Enter a name and a value in the empty table row to add a variable. Clear its checkbox to disable it. The **Secrets** tab masks values and keeps them out of exported environment files. Import accepts Reqon, YAML, environment JSON and `.env` files. The actions menu exports, duplicates or deletes an environment; deletion keeps a local recovery snapshot.

Use `{{baseUrl}}/users/{{userId}}` in an HTTP URL or `{{host}}:{{port}}` in a gRPC host. Variables also work in headers and bodies. Typing `{{` offers variable names: click one, or use the arrow keys and Enter/Tab. Defined variables are highlighted; unresolved variables appear red. Requests keep the original template, so changing the active environment changes the next request.

Variables may reference other variables. Disabled rows do not participate in substitution. **No Environment** clears the workspace and collection selections; global and request variables remain available. Names can be entered as `baseUrl` or `{{baseUrl}}`.

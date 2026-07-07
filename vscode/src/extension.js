const vscode = require('vscode');
const { LanguageClient } = require('vscode-languageclient/node');

let client;

/**
 * @param {vscode.ExtensionContext} context
 */
function activate(context) {
    const config = vscode.workspace.getConfiguration('rhas');
    const executablePath = config.get('executablePath') || 'rhas';

    // LSPサーバー起動設定（標準入出力経由で通信）
    const serverOptions = {
        run: { command: executablePath, args: ['--lsp'] },
        debug: { command: executablePath, args: ['--lsp'] }
    };

    // クライアント動作設定
    const clientOptions = {
        // has 言語およびファイルスキームを対象とする
        documentSelector: [{ scheme: 'file', language: 'has' }]
    };

    // Language Clientの生成・開始
    client = new LanguageClient(
        'rhasLanguageServer',
        'Rhas Language Server',
        serverOptions,
        clientOptions
    );

    client.start();
}

function deactivate() {
    if (!client) {
        return undefined;
    }
    return client.stop();
}

module.exports = {
    activate,
    deactivate
};

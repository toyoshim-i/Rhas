const vscode = require('vscode');
const { LanguageClient } = require('vscode-languageclient/node');
const fs = require('fs');
const path = require('path');
const { execSync } = require('child_process');

let client;

/**
 * @param {vscode.ExtensionContext} context
 */
function activate(context) {
    const executablePath = findExecutable(context);

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

/**
 * アセンブラの実行可能バイナリを自動探索する
 * @param {vscode.ExtensionContext} context 
 * @returns {string} 
 */
function findExecutable(context) {
    const config = vscode.workspace.getConfiguration('rhas');
    const configPath = config.get('executablePath');
    
    // 1. 設定項目がデフォルト以外かつ実在すれば最優先で使用
    if (configPath && configPath !== 'rhas') {
        if (fs.existsSync(configPath)) {
            return configPath;
        }
    }

    // 2. システムPATH上のグローバルコマンド "rhas" を探索
    if (commandExists('rhas')) {
        return 'rhas';
    }

    // 3. ローカル開発環境のビルド成果物を探索 (target/debug/rhas または target/release/rhas)
    const workspaceFolders = vscode.workspace.workspaceFolders;
    if (workspaceFolders) {
        const rootPath = workspaceFolders[0].uri.fsPath;
        const debugPath = path.join(rootPath, 'target', 'debug', binaryName());
        if (fs.existsSync(debugPath)) {
            return debugPath;
        }
        const releasePath = path.join(rootPath, 'target', 'release', binaryName());
        if (fs.existsSync(releasePath)) {
            return releasePath;
        }
    }

    // 4. 配布パッケージ等の配置構造を探索 (拡張機能フォルダの親・同階層)
    const siblingPath = path.join(context.extensionPath, '..', binaryName());
    if (fs.existsSync(siblingPath)) {
        return siblingPath;
    }
    const parentSiblingPath = path.join(context.extensionPath, '..', '..', binaryName());
    if (fs.existsSync(parentSiblingPath)) {
        return parentSiblingPath;
    }

    // フォールバック
    return 'rhas';
}

function binaryName() {
    return process.platform === 'win32' ? 'rhas.exe' : 'rhas';
}

function commandExists(command) {
    try {
        const cmd = process.platform === 'win32' ? `where ${command}` : `which ${command}`;
        execSync(cmd, { stdio: 'ignore' });
        return true;
    } catch (e) {
        return false;
    }
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

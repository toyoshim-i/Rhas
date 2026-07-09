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
    startLanguageServer(context);

    // 設定変更（実行パスやインクルードパス）を監視し、自動でサーバーを再起動
    context.subscriptions.push(vscode.workspace.onDidChangeConfiguration(e => {
        if (e.affectsConfiguration('rhas')) {
            restartLanguageServer(context);
        }
    }));
}

function startLanguageServer(context) {
    const executablePath = findExecutable(context);

    // インクルードパス設定を取得して引数に追加
    const config = vscode.workspace.getConfiguration('rhas');
    const includePaths = config.get('includePaths') || [];
    const args = ['--lsp'];

    // ワークスペースのルートパスを取得
    const workspaceFolders = vscode.workspace.workspaceFolders;
    const workspaceRoot = workspaceFolders ? workspaceFolders[0].uri.fsPath : null;

    for (const p of includePaths) {
        if (workspaceRoot && !path.isAbsolute(p)) {
            // 相対パスの場合はワークスペースのルート基準で絶対パスに解決
            args.push('-i', path.resolve(workspaceRoot, p));
        } else {
            args.push('-i', p);
        }
    }

    // LSPサーバー起動設定（標準入出力経由で通信）
    const serverOptions = {
        run: { command: executablePath, args: args },
        debug: { command: executablePath, args: args }
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

function restartLanguageServer(context) {
    if (client) {
        const oldClient = client;
        client = null;
        oldClient.stop().then(() => {
            startLanguageServer(context);
        }).catch(err => {
            console.error('Failed to stop language server during restart:', err);
            startLanguageServer(context);
        });
    } else {
        startLanguageServer(context);
    }
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

    // 2. 同梱されたプラットフォーム固有のバイナリを優先使用（自己完結配布用）
    const platform = process.platform; // 'win32', 'linux', 'darwin'
    const arch = process.arch;         // 'x64', 'arm64'
    let binName = '';
    
    if (platform === 'win32' && arch === 'x64') {
        binName = 'rhas-win32-x64.exe';
    } else if (platform === 'linux' && arch === 'x64') {
        binName = 'rhas-linux-x64';
    } else if (platform === 'darwin') {
        if (arch === 'arm64') {
            binName = 'rhas-darwin-arm64';
        } else if (arch === 'x64') {
            binName = 'rhas-darwin-x64';
        }
    }
    
    if (binName) {
        const bundledPath = path.join(context.extensionPath, 'bin', binName);
        if (fs.existsSync(bundledPath)) {
            // Windows 以外の場合、実行可能パーミッション (chmod +x) を保証
            if (platform !== 'win32') {
                try {
                    fs.chmodSync(bundledPath, '755');
                } catch (e) {
                    console.error('Failed to set executable permissions on bundled binary:', e);
                }
            }
            return bundledPath;
        }
    }

    // 3. 開発用フォールバック: ワークスペースのビルド成果物 (target/debug/rhas または target/release/rhas)
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

    // 4. システムPATH上のグローバルコマンド "rhas" を探索
    if (commandExists('rhas')) {
        return 'rhas';
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

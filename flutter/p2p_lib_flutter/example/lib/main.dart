import 'dart:convert';
import 'dart:typed_data';

import 'package:flutter/material.dart';
import 'package:p2p_lib_flutter/p2p_lib_flutter.dart';

void main() {
  runApp(const ChatApp());
}

class ChatApp extends StatelessWidget {
  const ChatApp({super.key});

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'p2p_lib_flutter chat',
      theme: ThemeData(colorSchemeSeed: Colors.teal, useMaterial3: true),
      home: const HomePage(),
    );
  }
}

/// Landing screen: choose to host a server (and share its token) or join
/// an existing one by pasting its token. Mirrors the two Rust examples
/// `server.rs`/`client.rs`, combined into one screen.
class HomePage extends StatefulWidget {
  const HomePage({super.key});

  @override
  State<HomePage> createState() => _HomePageState();
}

class _HomePageState extends State<HomePage> {
  final _tokenController = TextEditingController();

  @override
  void dispose() {
    _tokenController.dispose();
    super.dispose();
  }

  void _hostServer() {
    Navigator.of(context).push(
      MaterialPageRoute(builder: (_) => const ServerChatPage()),
    );
  }

  void _joinServer() {
    final token = _tokenController.text.trim();
    if (token.isEmpty) return;
    Navigator.of(context).push(
      MaterialPageRoute(builder: (_) => ClientChatPage(connBlob: token)),
    );
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: const Text('p2p_lib_flutter chat')),
      body: Center(
        child: ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 480),
          child: Padding(
            padding: const EdgeInsets.all(24),
            child: Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                Text(
                  'Peer-to-peer chat over WireGuard + DERP, no account or '
                  'control plane required.',
                  style: Theme.of(context).textTheme.bodyMedium,
                ),
                const SizedBox(height: 24),
                FilledButton.icon(
                  onPressed: _hostServer,
                  icon: const Icon(Icons.wifi_tethering),
                  label: const Text('Host (start a server)'),
                ),
                const SizedBox(height: 32),
                const Divider(),
                const SizedBox(height: 8),
                Text('Or join a host', style: Theme.of(context).textTheme.titleMedium),
                const SizedBox(height: 8),
                TextField(
                  controller: _tokenController,
                  decoration: const InputDecoration(
                    labelText: 'Connection token',
                    border: OutlineInputBorder(),
                  ),
                  minLines: 1,
                  maxLines: 4,
                ),
                const SizedBox(height: 12),
                OutlinedButton.icon(
                  onPressed: _joinServer,
                  icon: const Icon(Icons.login),
                  label: const Text('Join'),
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}

/// One chat line, either sent by us or received from the peer.
class _ChatLine {
  final String text;
  final bool mine;
  const _ChatLine(this.text, {required this.mine});
}

/// Shared chat UI: a scrolling message list plus a text field, driven by a
/// [Conn] that the two concrete pages ([ServerChatPage]/[ClientChatPage])
/// set up differently (accept vs. dial).
class _ChatView extends StatefulWidget {
  final String title;
  final String? banner;
  final Future<Conn> Function() connect;

  const _ChatView({required this.title, this.banner, required this.connect});

  @override
  State<_ChatView> createState() => _ChatViewState();
}

class _ChatViewState extends State<_ChatView> {
  final _messages = <_ChatLine>[];
  final _inputController = TextEditingController();
  final _scrollController = ScrollController();

  Conn? _conn;
  String _status = 'Connecting...';
  bool _ready = false;

  @override
  void initState() {
    super.initState();
    _connect();
  }

  Future<void> _connect() async {
    try {
      final conn = await widget.connect();
      if (!mounted) {
        await conn.close();
        return;
      }
      setState(() {
        _conn = conn;
        _status = 'Connected';
        _ready = true;
      });
      _readLoop(conn);
    } catch (e) {
      if (!mounted) return;
      setState(() => _status = 'Failed: $e');
    }
  }

  Future<void> _readLoop(Conn conn) async {
    while (mounted && !conn.isClosed) {
      List<int> bytes;
      try {
        bytes = await conn.read(4096);
      } catch (e) {
        if (!mounted) return;
        setState(() => _status = 'Connection error: $e');
        return;
      }
      if (bytes.isEmpty) {
        if (!mounted) return;
        setState(() => _status = 'Peer disconnected');
        return;
      }
      final text = utf8.decode(bytes, allowMalformed: true);
      if (!mounted) return;
      setState(() => _messages.add(_ChatLine(text, mine: false)));
      _scrollToBottom();
    }
  }

  void _scrollToBottom() {
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (!_scrollController.hasClients) return;
      _scrollController.animateTo(
        _scrollController.position.maxScrollExtent,
        duration: const Duration(milliseconds: 200),
        curve: Curves.easeOut,
      );
    });
  }

  Future<void> _send() async {
    final text = _inputController.text;
    final conn = _conn;
    if (text.isEmpty || conn == null || conn.isClosed) return;
    _inputController.clear();
    setState(() => _messages.add(_ChatLine(text, mine: true)));
    _scrollToBottom();
    try {
      await conn.writeAll(Uint8List.fromList(utf8.encode('$text\n')));
    } catch (e) {
      if (!mounted) return;
      setState(() => _status = 'Send failed: $e');
    }
  }

  @override
  void dispose() {
    _conn?.close();
    _inputController.dispose();
    _scrollController.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: Text(widget.title)),
      body: Column(
        children: [
          if (widget.banner != null)
            Container(
              width: double.infinity,
              color: Theme.of(context).colorScheme.surfaceContainerHighest,
              padding: const EdgeInsets.all(12),
              child: SelectableText(widget.banner!),
            ),
          Padding(
            padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 4),
            child: Row(
              children: [
                Icon(
                  _ready ? Icons.check_circle : Icons.hourglass_top,
                  size: 16,
                  color: _ready ? Colors.green : null,
                ),
                const SizedBox(width: 6),
                Expanded(child: Text(_status, overflow: TextOverflow.ellipsis)),
              ],
            ),
          ),
          const Divider(height: 1),
          Expanded(
            child: ListView.builder(
              controller: _scrollController,
              padding: const EdgeInsets.all(12),
              itemCount: _messages.length,
              itemBuilder: (context, i) {
                final line = _messages[i];
                return Align(
                  alignment: line.mine ? Alignment.centerRight : Alignment.centerLeft,
                  child: Container(
                    margin: const EdgeInsets.symmetric(vertical: 4),
                    padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
                    decoration: BoxDecoration(
                      color: line.mine
                          ? Theme.of(context).colorScheme.primaryContainer
                          : Theme.of(context).colorScheme.secondaryContainer,
                      borderRadius: BorderRadius.circular(12),
                    ),
                    child: Text(line.text.trimRight()),
                  ),
                );
              },
            ),
          ),
          SafeArea(
            child: Padding(
              padding: const EdgeInsets.all(8),
              child: Row(
                children: [
                  Expanded(
                    child: TextField(
                      controller: _inputController,
                      enabled: _ready,
                      decoration: const InputDecoration(
                        hintText: 'Message',
                        border: OutlineInputBorder(),
                      ),
                      onSubmitted: (_) => _send(),
                    ),
                  ),
                  const SizedBox(width: 8),
                  IconButton.filled(
                    onPressed: _ready ? _send : null,
                    icon: const Icon(Icons.send),
                  ),
                ],
              ),
            ),
          ),
        ],
      ),
    );
  }
}

/// Hosts a server, waits for one client, then chats with it.
class ServerChatPage extends StatefulWidget {
  const ServerChatPage({super.key});

  @override
  State<ServerChatPage> createState() => _ServerChatPageState();
}

class _ServerChatPageState extends State<ServerChatPage> {
  Server? _server;
  String? _connBlob;

  @override
  void dispose() {
    _server?.close();
    super.dispose();
  }

  Future<Conn> _connect() async {
    final server = await Server.create();
    _server = server;
    await server.start();
    final blob = await server.connBlob();
    if (mounted) setState(() => _connBlob = blob);
    Conn? conn;
    while (conn == null) {
      conn = await server.accept(const Duration(seconds: 120));
    }
    return conn;
  }

  @override
  Widget build(BuildContext context) {
    return _ChatView(
      title: 'Hosting',
      banner: _connBlob == null
          ? 'Starting server...'
          : 'Share this token with your peer:\n$_connBlob',
      connect: _connect,
    );
  }
}

/// Connects to a host using a pasted connection token, then chats with it.
class ClientChatPage extends StatefulWidget {
  final String connBlob;
  const ClientChatPage({super.key, required this.connBlob});

  @override
  State<ClientChatPage> createState() => _ClientChatPageState();
}

class _ClientChatPageState extends State<ClientChatPage> {
  Client? _client;

  @override
  void dispose() {
    _client?.close();
    super.dispose();
  }

  Future<Conn> _connect() async {
    final client = await Client.create(widget.connBlob);
    _client = client;
    await client.ping(const Duration(seconds: 30));
    return client.dialTcpPort(0, const Duration(seconds: 30));
  }

  @override
  Widget build(BuildContext context) {
    return _ChatView(title: 'Joined', connect: _connect);
  }
}

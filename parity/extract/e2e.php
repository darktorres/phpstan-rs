<?php declare(strict_types = 1);

// The end-to-end suite's expected side (docs/design/scoreboard.md,
// "end-to-end").
//
// Usage: php e2e.php <phpstan-src> <phar> < projects.tsv > cases.jsonl
//
// projects.tsv holds one project per line: its name, its directory (the
// project at its release commit, its dependencies installed), its paths and
// its excluded paths, tab-separated; paths are relative to the project and
// separated by spaces. Each project is analysed at every level from 0 to 10
// by the phar as a user runs it, in the project's directory, so PHPStan
// finds the project's composer.json and autoloader: `phpstan analyse --debug
// --no-progress --error-format=json -c level-<n>.neon`, the config holding
// only the level, the paths, the excluded paths and a tmpDir of its own.
// Without phpVersion in it, PHPStan takes the PHP versions from
// composer.json. --debug analyses in the one process, which changes no
// finding, and prints each analysed file's path first, which names the
// cases. The phar restarts itself with the turbo extension beside it.
//
// The runs go to $PARITY_JOBS processes at a time. Each run's config,
// tmpDir and output are under $PARITY_WORK, emptied first.
//
// Output, in id order, one line per project and level, its id
// "<name>@<level>", with the exit code and the errors that belong to no
// file; and one per analysed file and level, its id "<name>/<path>@<level>",
// with the file's findings sorted by line, identifier and message. A trait's
// findings are reported once per class using it, which each finding names as
// its context (null outside traits), sorted first. Paths are written relative
// to the project (OM-2). A run that printed no JSON has only
// its project line, whose errors say why.

$src = $argv[1] ?? null;
$phar = $argv[2] ?? null;
if ($src === null || $phar === null) {
	throw new RuntimeException('usage: e2e.php <phpstan-src> <phar>');
}
set_error_handler(static function (int $severity, string $message, string $file, int $line): never {
	throw new ErrorException($message, 0, $severity, $file, $line);
});

const LEVELS = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
const FLAGS = JSON_THROW_ON_ERROR | JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE | JSON_INVALID_UTF8_SUBSTITUTE;

function removeTree(string $dir): void
{
	if (!is_dir($dir)) {
		return;
	}
	foreach (new RecursiveIteratorIterator(new RecursiveDirectoryIterator($dir, FilesystemIterator::SKIP_DOTS), RecursiveIteratorIterator::CHILD_FIRST) as $entry) {
		$entry->isDir() && !$entry->isLink() ? rmdir($entry->getPathname()) : unlink($entry->getPathname());
	}
	rmdir($dir);
}

/** The NEON list of `$paths` under `$dir`, indented by `$indent` tabs. */
function neonList(string $dir, string $paths, int $indent): string
{
	$out = '';
	foreach (preg_split('/ +/', trim($paths), -1, PREG_SPLIT_NO_EMPTY) ?: [] as $path) {
		$out .= str_repeat("\t", $indent) . '- ' . json_encode(rtrim("$dir/$path", '/.'), FLAGS) . "\n";
	}
	return $out;
}

/**
 * Starts the phar on `$project` at `$level` in `$run`, which it makes.
 *
 * @param array{string, string, string, string} $project
 * @return resource
 */
function start(array $project, int $level, string $phar, string $run)
{
	[, $dir, $paths, $exclude] = $project;
	mkdir($run, 0777, true);
	$config = "parameters:\n\tlevel: $level\n\tpaths:\n" . neonList($dir, $paths, 2);
	if (trim($exclude) !== '') {
		$config .= "\texcludePaths:\n\t\tanalyseAndScan:\n" . neonList($dir, $exclude, 3);
	}
	$config .= "\ttmpDir: " . json_encode("$run/tmp", FLAGS) . "\n";
	file_put_contents("$run/level.neon", $config);
	$command = [
		PHP_BINARY,
		'-d', 'memory_limit=-1',
		$phar, 'analyse', '--debug', '--no-progress', '--error-format=json', '-c', "$run/level.neon",
	];
	$process = proc_open($command, [0 => ['file', '/dev/null', 'r'], 1 => ['file', "$run/out", 'w'], 2 => ['file', "$run/err", 'w']], $pipes, $dir);
	if ($process === false) {
		throw new RuntimeException("cannot start the phar in $dir");
	}
	return $process;
}

/**
 * The records of a finished run, keyed for sorting.
 *
 * @param array{string, string, string, string} $project
 * @return array<string, array<string, mixed>>
 */
function records(array $project, int $level, string $run, int $code): array
{
	[$name, $dir] = $project;
	$out = (string) file_get_contents("$run/out");
	$start = strpos($out, '{');
	$json = $start === false ? null : json_decode(substr($out, $start), true);
	$relative = static fn (string $text): string => str_replace($dir . '/', '', $text);
	$key = static fn (string $id): string => $id . "\0" . sprintf('%02d', $level);
	if (!is_array($json)) {
		$why = preg_match('/^Uncaught .*$/m', $out, $match) === 1
			? (string) preg_replace('~phar://\S*/phpstan\.phar/~', 'phar://phpstan.phar/', $match[0])
			: trim((string) file_get_contents("$run/err"));
		return [$key($name) => ['id' => "$name@$level", 'exit' => $code, 'other' => ['no JSON output: ' . $relative($why)]]];
	}
	$records = [$key($name) => ['id' => "$name@$level", 'exit' => $code, 'other' => array_map($relative, $json['errors'] ?? [])]];
	$findings = [];
	foreach ($json['files'] ?? [] as $file => $messages) {
		// A trait's findings are keyed by its file and each class using it.
		$context = null;
		if (preg_match('/^(.*) \(in context of class (.*)\)$/', $file, $match) === 1) {
			[, $file, $context] = $match;
		}
		foreach ($messages['messages'] as $message) {
			$findings[$relative($file)][] = [
				'context' => $context,
				'line' => $message['line'] ?? null,
				'identifier' => $message['identifier'] ?? null,
				'message' => $relative($message['message']),
				'tip' => isset($message['tip']) ? $relative($message['tip']) : null,
				'ignorable' => $message['ignorable'] ?? null,
			];
		}
	}
	foreach (explode("\n", substr($out, 0, (int) $start)) as $line) {
		// --debug also prints a line per rule run on the collected data.
		if (!str_starts_with($line, $dir . '/')) {
			continue;
		}
		$file = $relative(rtrim($line));
		$errors = $findings[$file] ?? [];
		unset($findings[$file]);
		usort($errors, static fn (array $a, array $b): int => [$a['context'] ?? '', $a['line'], $a['identifier'], $a['message']] <=> [$b['context'] ?? '', $b['line'], $b['identifier'], $b['message']]);
		$records[$key("$name/$file")] = ['id' => "$name/$file@$level", 'errors' => $errors];
	}
	if ($findings !== []) {
		throw new RuntimeException(sprintf('%s@%d reported findings in files it did not list: %s', $name, $level, implode(', ', array_keys($findings))));
	}
	return $records;
}

$work = getenv('PARITY_WORK') ?: throw new RuntimeException('PARITY_WORK is not set');
$jobs = max(1, (int) (getenv('PARITY_JOBS') ?: 1));
removeTree($work);
mkdir($work, 0777, true);

$runs = [];
while (($line = fgets(STDIN)) !== false) {
	$line = rtrim($line, "\n");
	if ($line === '') {
		continue;
	}
	$project = explode("\t", $line) + ['', '', '', ''];
	foreach (LEVELS as $level) {
		$runs[] = [array_slice($project, 0, 4), $level];
	}
}

$records = [];
$running = [];
while ($runs !== [] || $running !== []) {
	while ($runs !== [] && count($running) < $jobs) {
		[$project, $level] = array_shift($runs);
		$run = sprintf('%s/%s-%02d', $work, $project[0], $level);
		$running[] = [start($project, $level, $phar, $run), $project, $level, $run];
	}
	usleep(20000);
	foreach ($running as $k => [$process, $project, $level, $run]) {
		$status = proc_get_status($process);
		if ($status['running']) {
			continue;
		}
		proc_close($process);
		unset($running[$k]);
		$records += records($project, $level, $run, $status['exitcode']);
	}
}

ksort($records, SORT_STRING);
foreach ($records as $record) {
	echo json_encode($record, FLAGS), "\n";
}

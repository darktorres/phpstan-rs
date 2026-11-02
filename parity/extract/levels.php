<?php declare(strict_types = 1);

// The level oracle's expected side (docs/design/scoreboard.md, "level
// oracle").
//
// Usage: php levels.php <phpstan-src> <phar> < files.tsv > cases.jsonl
//
// <phar> is the phar of the phpstan/phpstan package, beside its turbo-ext/
// directory: the workers load the turbo extension as the package's
// bin/phpstan does, which takes about a fifth off a run's CPU time and
// changes no record.
//
// files.tsv holds one rule data file per line: its id, a tab, and the file.
// Each file is analysed at every level from 0 to 10, each run on its own
// (CA-4): a project holding only that file, and a config with the level and
// phpVersion as a range.
//
// The runs go to worker processes, $PARITY_JOBS at a time, each taking a
// batch of runs (spike S4). Each worker slot has one project directory under
// $PARITY_WORK, with a level-<n>.neon per level, its tmpDir, and the one
// data file of the current run. A worker loads the phar once, as its
// bin/phpstan does, and then runs PHPStan's own console application per
// run, as `phpstan analyse --debug --no-progress --error-format=json -c
// level-<n>.neon <file>` would in that directory. --debug analyses in the
// one process instead of starting workers, and never uses the result cache.
// Since the file is named on the command line, not in the config, the
// compiled container depends only on the level, so the slot compiles 11 and
// reuses them. php's opcache file cache under $PARITY_WORK keeps the phar's
// compiled code between workers, for this extraction only. None of this
// changes what PHPStan reports: the records equal those of one phar
// process per run with the file in phpstan.neon's paths, byte for byte.
//
// Progress goes to $PARITY_WORK/levels.log, a line per finished batch and
// per worker that died, so a run can be followed while it goes.
//
// A worker that dies keeps the runs it finished. The run it died on is run
// again alone, and if it dies alone too, it is recorded with that worker's
// exit code and no output; the rest of the batch is queued again.
//
// One output line per file and level, in id order: its id
// ("<file id>@<level>"), the exit code, the findings (file errors, sorted by
// line, identifier and message) and the other errors. Paths are written
// relative to the project (OM-2), so the records don't depend on where the
// work directory is.

if (($argv[1] ?? null) === '--worker') {
	worker($argv[2], $argv[3], $argv[4]);
	exit(0);
}

$src = $argv[1] ?? null;
$phar = $argv[2] ?? null;
if ($src === null || $phar === null) {
	throw new RuntimeException('usage: levels.php <phpstan-src> <phar>');
}
set_error_handler(static function (int $severity, string $message, string $file, int $line): never {
	throw new ErrorException($message, 0, $severity, $file, $line);
});

const LEVELS = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
const BATCH = 500;
const FLAGS = JSON_THROW_ON_ERROR | JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE | JSON_INVALID_UTF8_SUBSTITUTE;

/**
 * Runs PHPStan's analyse command in `$project` once for each `<file>\t<level>`
 * line on stdin, in this process. The n-th run's `out`, `err` and `exit` go
 * to `$runs/<n>`.
 */
function worker(string $phar, string $project, string $runs): void
{
	// What the phar's bin/phpstan sets before it starts.
	error_reporting(E_ALL & ~E_DEPRECATED);
	ini_set('display_errors', 'stderr');
	if (ini_get('zend.assertions') !== '-1') {
		ini_set('zend.assertions', '0');
	}
	define('__PHPSTAN_RUNNING__', true);
	$loader = require "phar://$phar/vendor/autoload.php";
	// bin/phpstan also calls trustOwnTypesIfSuitable(), which does nothing
	// under --debug.
	\PHPStan\Turbo\TurboExtensionEnabler::activateIfCompatible($phar);
	if (!\PHPStan\Turbo\TurboExtensionEnabler::isActive()) {
		throw new RuntimeException("the turbo extension is loaded but $phar does not activate it");
	}
	require_once "phar://$phar/preload.php";
	// The projects have no vendor/ of their own, so no autoloader is added.
	$loader->unregister();
	$loader->register(true);
	$GLOBALS['__phpstanAutoloadFunctions'] = [];

	// The phar's Symfony classes live under a build-specific prefix.
	$prefix = strstr((string) get_parent_class(\PHPStan\Command\AnalyseCommand::class), '\\Symfony\\', true);
	$console = $prefix . '\\Symfony\\Component\\Console';
	class_alias("$console\\Output\\StreamOutput", 'LevelsStreamOutput');
	class_alias("$console\\Output\\ConsoleOutputInterface", 'LevelsConsoleOutputInterface');
	class_alias("$console\\Output\\OutputInterface", 'LevelsOutputInterface');
	class_alias("$console\\Output\\ConsoleSectionOutput", 'LevelsConsoleSectionOutput');
	if (!class_exists('LevelsSplitOutput', false)) {
		// stdout and stderr kept apart, as the console's ConsoleOutput keeps
		// them: the JSON goes to stdout and notes to stderr.
		final class LevelsSplitOutput extends LevelsStreamOutput implements LevelsConsoleOutputInterface
		{

			private LevelsOutputInterface $error;

			/** @param resource $stdout @param resource $stderr */
			public function __construct($stdout, $stderr)
			{
				parent::__construct($stdout);
				$this->error = new LevelsStreamOutput($stderr);
			}

			public function getErrorOutput(): LevelsOutputInterface
			{
				return $this->error;
			}

			public function setErrorOutput(LevelsOutputInterface $error): void
			{
				$this->error = $error;
			}

			public function section(): LevelsConsoleSectionOutput
			{
				throw new LogicException('the analyse command uses no sections');
			}

		}
	}

	$version = \PHPStan\Internal\ComposerHelper::getPhpStanVersion();
	chdir($project);
	$n = 0;
	while (($line = fgets(STDIN)) !== false) {
		[$file, $level] = explode("\t", rtrim($line, "\n"));
		foreach (glob($project . '/*.php') ?: [] as $previous) {
			unlink($previous);
		}
		copy($file, $project . '/' . basename($file));
		$application = new ("$console\\Application")('PHPStan - PHP Static Analysis Tool', $version);
		$application->setAutoExit(false);
		$application->add(new \PHPStan\Command\AnalyseCommand([], microtime(true)));
		$input = new ("$console\\Input\\ArgvInput")(['phpstan', 'analyse', '--debug', '--no-progress', '--error-format=json', '-c', "level-$level.neon", basename($file)]);
		$run = sprintf('%s/%03d', $runs, $n++);
		mkdir($run);
		$stdout = fopen("$run/out", 'w');
		$stderr = fopen("$run/err", 'w');
		$code = $application->run($input, new LevelsSplitOutput($stdout, $stderr));
		fclose($stdout);
		fclose($stderr);
		file_put_contents("$run/exit", (string) $code);
		gc_collect_cycles();
	}
}

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

/**
 * Starts a worker on the batch, in the slot's project under $dir, which is
 * made on first use. The runs' outputs go to $dir/runs, emptied first.
 *
 * @param list<array{string, string, int}> $batch
 * @return array{resource, string, string} the process, the project and the runs directory
 */
function startBatch(string $dir, string $phar, string $turbo, string $opcache, array $batch): array
{
	$project = "$dir/project";
	if (!is_dir($project)) {
		mkdir($project, 0777, true);
		foreach (LEVELS as $level) {
			file_put_contents("$project/level-$level.neon", sprintf(
				"parameters:\n\tlevel: %d\n\tphpVersion:\n\t\tmin: 70100\n\t\tmax: 80599\n\ttmpDir: tmp\n",
				$level,
			));
		}
	}
	$runs = "$dir/runs";
	removeTree($runs);
	mkdir($runs);
	$lines = '';
	foreach ($batch as [, $file, $level]) {
		$lines .= "$file\t$level\n";
	}
	file_put_contents("$dir/batch.tsv", $lines);
	$command = [
		PHP_BINARY,
		'-d', 'memory_limit=-1',
		// php.ini files differ on it, as they do for the extractors.
		'-d', 'short_open_tag=1',
		'-d', 'opcache.enable_cli=1',
		'-d', 'opcache.file_cache=' . $opcache,
		'-d', 'opcache.file_cache_only=1',
		'-d', 'extension=' . $turbo,
		'-d', 'phpstan.turboExtensionPath=' . $turbo,
		__FILE__, '--worker', $phar, $project, $runs,
	];
	$process = proc_open($command, [0 => ['file', "$dir/batch.tsv", 'r'], 1 => ['file', "$dir/stdout", 'w'], 2 => ['file', "$dir/stderr", 'w']], $pipes);
	if ($process === false) {
		throw new RuntimeException("cannot start a worker in $dir");
	}
	return [$process, $project, $runs];
}

/** The record of a finished run, whose outputs are in $run. */
function record(string $id, int $level, string $project, string $run, int $code): array
{
	$out = is_file($run . '/out') ? (string) file_get_contents($run . '/out') : '';
	$err = is_file($run . '/err') ? (string) file_get_contents($run . '/err') : '';
	// --debug prints each analysed file's path before the JSON.
	$start = strpos($out, '{');
	$json = $start === false ? null : json_decode(substr($out, $start), true);
	$relative = static fn (string $text): string => str_replace($project . '/', '', $text);
	if (!is_array($json)) {
		// An uncaught exception is printed on stdout, the message line and
		// then the trace; the message is kept, with the phar's location
		// spelled without the directory it was fetched to.
		$why = preg_match('/^Uncaught .*$/m', $out, $match) === 1
			? (string) preg_replace('~phar://\S*/phpstan\.phar/~', 'phar://phpstan.phar/', $match[0])
			: trim($err);
		return ['id' => "$id@$level", 'exit' => $code, 'errors' => [], 'other' => ['no JSON output: ' . $relative($why)]];
	}
	$errors = [];
	foreach ($json['files'] ?? [] as $messages) {
		foreach ($messages['messages'] as $message) {
			$errors[] = [
				'line' => $message['line'] ?? null,
				'identifier' => $message['identifier'] ?? null,
				'message' => $relative($message['message']),
				'tip' => isset($message['tip']) ? $relative($message['tip']) : null,
				'ignorable' => $message['ignorable'] ?? null,
			];
		}
	}
	usort($errors, static fn (array $a, array $b): int => [$a['line'], $a['identifier'], $a['message']] <=> [$b['line'], $b['identifier'], $b['message']]);
	return [
		'id' => "$id@$level",
		'exit' => $code,
		'errors' => $errors,
		'other' => array_map($relative, $json['errors'] ?? []),
	];
}

$work = getenv('PARITY_WORK') ?: throw new RuntimeException('PARITY_WORK is not set');
$jobs = max(1, (int) (getenv('PARITY_JOBS') ?: 1));
$opcache = $work . '/opcache';
require_once "phar://$phar/src/Turbo/TurboExtensionSelector.php";
$turbo = \PHPStan\Turbo\TurboExtensionSelector::findExtension($phar)
	?? throw new RuntimeException("no turbo extension for this php next to $phar");
// What a previous extraction left is removed first: compiled containers and
// opcache's compiled code fail, or change, under another php setup, and the
// cache key sees only php -v, not the extensions loaded beside it.
removeTree($work);
foreach ([$work, $opcache] as $dir) {
	mkdir($dir, 0777, true);
}

$runs = [];
while (($line = fgets(STDIN)) !== false) {
	$line = rtrim($line, "\n");
	if ($line === '') {
		continue;
	}
	[$id, $file] = explode("\t", $line, 2);
	foreach (LEVELS as $level) {
		$runs[] = [$id, $file, $level];
	}
}

// Each queued batch is a list of runs and whether it is a run being retried
// alone after its worker died.
$queue = array_map(static fn (array $batch): array => [$batch, false], array_chunk($runs, BATCH));
$log = fopen("$work/levels.log", 'a') ?: throw new RuntimeException("cannot open $work/levels.log");
$started = hrtime(true);
$say = static function (string $line) use ($log, $started): void {
	fwrite($log, sprintf("%7.1fs %s\n", (hrtime(true) - $started) / 1e9, $line));
	fflush($log);
};
$say(sprintf('%d runs of %d files, in batches of %d, %d workers', count($runs), intdiv(count($runs), count(LEVELS)), BATCH, $jobs));
$deaths = 0;
$records = [];
$running = [];
$free = range(0, $jobs - 1);
while ($queue !== [] || $running !== []) {
	while ($queue !== [] && $free !== []) {
		[$batch, $alone] = array_shift($queue);
		$slot = array_shift($free);
		[$process, $project, $runsDir] = startBatch("$work/worker-$slot", $phar, $turbo, $opcache, $batch);
		$running[$slot] = [$process, $project, $runsDir, $batch, $alone];
	}
	usleep(20000);
	foreach ($running as $slot => [$process, $project, $runsDir, $batch, $alone]) {
		$status = proc_get_status($process);
		if ($status['running']) {
			continue;
		}
		proc_close($process);
		unset($running[$slot]);
		$free[] = $slot;
		foreach ($batch as $k => [$id, , $level]) {
			$run = sprintf('%s/%03d', $runsDir, $k);
			if (is_file("$run/exit")) {
				$record = record($id, $level, $project, $run, (int) file_get_contents("$run/exit"));
				$records[$id . "\0" . sprintf('%02d', $level)] = $record;
				if (str_starts_with($record['other'][0] ?? '', 'no JSON output')) {
					$say(sprintf('no JSON output from %s@%d (exit %d), worker %d', $id, $level, $record['exit'], $slot));
				}
				continue;
			}
			// The worker died on this run.
			$deaths++;
			$stderr = trim((string) @file_get_contents("$work/worker-$slot/stderr"));
			$say(sprintf(
				'worker %d died (exit %d) on %s@%d%s: %s',
				$slot,
				$status['exitcode'],
				$id,
				$level,
				$alone ? ', run alone' : '',
				substr(preg_replace('/\s+/', ' ', $stderr) ?? '', -300),
			));
			if ($alone) {
				$records[$id . "\0" . sprintf('%02d', $level)] = record($id, $level, $project, $run, $status['exitcode']);
			} else {
				$queue[] = [[$batch[$k]], true];
				$rest = array_slice($batch, $k + 1);
				if ($rest !== []) {
					$queue[] = [$rest, false];
				}
			}
			break;
		}
		$say(sprintf('worker %d finished a batch of %d: %d of %d runs recorded', $slot, count($batch), count($records), count($runs)));
	}
}
$say(sprintf('done: %d records, %d worker deaths', count($records), $deaths));

ksort($records, SORT_STRING);
foreach ($records as $record) {
	echo json_encode($record, FLAGS), "\n";
}

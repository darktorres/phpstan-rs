<?php
// The recorder's own probes, one child per probe of each parameter, as unit.php runs them, to classify what
// each probe throws. The population is the committed table's builtins that a regular expression matches
// (the same lowercased names the recorder uses). Each line of OUT: builtin, position, probe, the result
// (ok, threw:CLASS:MESSAGE, killed:SIGNAL), and the number of sink entries.
// Run inside the recorder's sandbox: php probe-scan.php TABLE REGEX OUT

require_once __DIR__ . '/../lib/seen.php';
require_once __DIR__ . '/../lib/probes.php';
require_once __DIR__ . '/../lib/plan.php';

[, $table, $regex, $out] = $argv;

function sink_count(): int
{
    return count(array_filter(array_keys(Seen::$entries), static fn (string $k): bool => !str_contains($k, "\treached\t") && !str_contains($k, "\treturned\t")));
}

$names = [];
foreach (file($table, FILE_IGNORE_NEW_LINES) as $line) {
    $fields = explode("\t", $line);
    if (($fields[2] ?? '') === 'probed' && preg_match($regex, $fields[0]) === 1) {
        $names[$fields[0]] = true;
    }
}

$fh = fopen($out, 'w');
foreach (array_keys($names) as $name) {
    if (str_contains($name, '::')) {
        [$class, $method] = explode('::', $name, 2);
        if (!class_exists($class)) {
            continue;
        }
        $reflection = new ReflectionMethod($class, $method);
        if ($reflection->isStatic() || $reflection->isAbstract()) {
            continue;
        }
        $targets = [['method', $class, $method, $reflection]];
    } elseif (function_exists($name)) {
        $targets = [['function', $name, '', new ReflectionFunction($name)]];
    } else {
        continue;
    }
    [$kind, $fn, $method, $reflection] = $targets[0];
    $label = $kind === 'function' ? $fn : "$fn::$method";
    $params = $reflection->getParameters();
    $makers = $kind === 'method' ? makers(new ReflectionClass($fn)) : [];
    foreach ($params as $at => $param) {
        foreach (probes_for($param) as [$probe, $make]) {
            $pipe = stream_socket_pair(STREAM_PF_UNIX, STREAM_SOCK_STREAM, STREAM_IPPROTO_IP);
            $pid = pcntl_fork();
            if ($pid === 0) {
                fclose($pipe[0]);
                pcntl_alarm(3);
                $dir = sys_get_temp_dir() . '/probe-scan-' . getmypid();
                @mkdir($dir);
                @chdir($dir);
                $value = $make();
                Seen::$entries = [];
                Seen::$where = 'x';
                try {
                    $args = arguments($params, $at, $value);
                    Seen::add('reached');
                    if ($kind === 'function') {
                        invoke_arguments($params, $at, $value, static fn (array $a): mixed => $fn(...$a));
                    } else {
                        // The recorder's own makers, so an object is made as the recorder makes it.
                        $object = method_receiver(new ReflectionClass($fn), $method, $makers[0][1]);
                        if ($object === null) {
                            throw new RuntimeException('no object');
                        }
                        invoke_arguments($params, $at, $value, static fn (array $a): mixed => $object->$method(...$a));
                    }
                    $result = 'ok';
                } catch (Throwable $e) {
                    $result = 'threw:' . get_class($e) . ':' . strtok($e->getMessage(), "\n");
                }
                fwrite($pipe[1], json_encode([$label, $at, $probe, $result, sink_count()]) . "\n");
                exit(0);
            }
            fclose($pipe[1]);
            pcntl_waitpid($pid, $status);
            $line = stream_get_contents($pipe[0]);
            fclose($pipe[0]);
            if ($line === '' || $line === false) {
                $result = pcntl_wifsignaled($status) ? 'killed:' . pcntl_wtermsig($status) : 'no-output';
                fwrite($fh, json_encode([$label, $at, $probe, $result, 0]) . "\n");
            } else {
                fwrite($fh, trim($line) . "\n");
            }
        }
    }
}
fclose($fh);
echo "done\n";

<?php
namespace Psr\Log {
    interface LoggerInterface {
        public function log($level, string|\Stringable $message, array $context = []): void;
        public function error(string|\Stringable $message, array $context = []): void;
        public function info(string|\Stringable $message, array $context = []): void;
    }
}
namespace App {
use Psr\Log\LoggerInterface;
enum Kind { case A; }
function f(LoggerInterface $logger, string $name, int $id, Kind $kind, array $data, string $level): void {
    $logger->info('User {name} logged in', ['name' => $name]);
    $logger->info('User {name} logged in');
    $logger->info('User {name} logged in', ['other' => $name]);
    $logger->info("User $name logged in");
    $logger->info('User ' . $id . ' logged in');
    $logger->info('Kind ' . $kind->name);
    $logger->info($name);
    $logger->info('Hello {{name}}', ['name' => $name]);
    $logger->info('Bad {na me}');
    $logger->log('warning', 'ok');
    $logger->log('fatal', 'bad level');
    $logger->log($level, 'dynamic level');
    $logger->info('ctx', ['' => 1, 5 => 2]);
    $logger->error('failed', ['exception' => 'text']);
    $logger->info('Data {data}', ['data' => $data]);
}
}

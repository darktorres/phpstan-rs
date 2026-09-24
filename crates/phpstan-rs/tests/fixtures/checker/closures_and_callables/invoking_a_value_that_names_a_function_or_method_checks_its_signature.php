<?php
            class Svc {
                public function run(int $n): void {}
                public static function make(string $s): void {}
                private function secret(): void {}
                public function __invoke(int $x): void {}
            }
            function f(Svc $svc): void {
                $a = 'strlen';
                $a();
                $b = [$svc, 'run'];
                $b('x');
                $c = 'Svc::make';
                $c();
                $d = ['Svc', 'make'];
                $d('ok');
                $e = [$svc, 'secret'];
                $e();
                $svc('nope');
            }
        
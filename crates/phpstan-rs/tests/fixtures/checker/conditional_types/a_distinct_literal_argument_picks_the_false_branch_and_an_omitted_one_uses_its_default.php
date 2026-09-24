<?php
            use function PHPStan\Testing\assertType;
            class Notification {}
            class Sender {
                /** @return ($action is 'send' ? list<string> : list<Notification>) */
                public function send(string $action = 'list_to_send'): array { return []; }
            }
            function f(Sender $s): void {
                assertType('list<Notification>', $s->send('list_to_send'));
                assertType('list<Notification>', $s->send());
                assertType('list<string>', $s->send('send'));
            }
        
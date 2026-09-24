<?php
            interface HandlerInterface {}
            class SafeEvent {}
            class DangerousEvent {}
            function dispatch(SafeEvent|DangerousEvent $e) {}
            class MyHandler implements HandlerInterface {
                public function f(): void {
                    dispatch(new DangerousEvent());
                    dispatch(new SafeEvent());
                }
            }
            class Other {
                public function f(): void {
                    dispatch(new DangerousEvent());
                }
            }
        
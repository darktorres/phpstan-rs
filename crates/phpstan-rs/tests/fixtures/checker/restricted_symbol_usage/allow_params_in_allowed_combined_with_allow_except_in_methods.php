<?php
            class SafeUrl {}
            class OtherUrl {}
            class Controller {
                public function redirect(SafeUrl|OtherUrl $url): void {}
                public function handleRequest(): void {
                    $this->redirect(new SafeUrl());
                    $this->redirect(new OtherUrl());
                }
                public function other(): void {
                    $this->redirect(new OtherUrl());
                }
            }
        
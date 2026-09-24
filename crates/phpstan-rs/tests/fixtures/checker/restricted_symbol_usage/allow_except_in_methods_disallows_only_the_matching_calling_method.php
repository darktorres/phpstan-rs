<?php
            class Controller {
                public function redirect(): void {}
                public function handleRequest(): void { $this->redirect(); }
                public function other(): void { $this->redirect(); }
            }
        
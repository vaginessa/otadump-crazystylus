import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/tauri";
import {
  Button,
  Divider,
  Field,
  FluentProvider,
  Input,
  ProgressBar,
  Text,
  useId,
  webLightTheme,
} from "@fluentui/react-components";
import { open } from "@tauri-apps/api/dialog";
import { Event, listen } from "@tauri-apps/api/event";

type Message = Progress | Error | Completed;

type Progress = {
  kind: "Progress";
  value: number;
};

type Error = {
  kind: "Error";
  message: string;
};

type Completed = {
  kind: "Completed";
};

function App() {
  useEffect(() => {
    const unlisten = listen("reporter", (e) => {
      console.log(e.payload);
    });

    return () => {
      unlisten.then((f) => f());
    };
  }, []);

  const [inputFile, setInputFile] = useState("");
  const [outputDir, setOutputDir] = useState("");

  const inputFileId = useId("input-file");
  const outputDirId = useId("output-dir");

  return (
    <FluentProvider
      css={{
        display: "flex",
        flexDirection: "column",
      }}
      theme={webLightTheme}
    >
      <div css={{ flexDirection: "row" }}>
        <Text size={700}>otadump</Text>
        <Text css={{ verticalAlign: "sub" }} size={100}>
          v0.1.2
        </Text>
      </div>
      <Text size={100}>© Kartik Sharma & Ajeet D'Souza</Text>

      <Divider appearance="strong" />

      <Field label="Payload file">
        <div
          css={{
            display: "flex",
            flexDirection: "row",
          }}
        >
          <Input
            css={{ flexGrow: 1 }}
            id={inputFileId}
            onChange={(e) => setInputFile(e.currentTarget.value)}
            value={inputFile}
          />
          <Button
            appearance="outline"
            onClick={async () => {
              const inputFile = await open();
              if (inputFile === null) {
                return;
              }
              if (Array.isArray(inputFile)) {
                throw new Error("expected a single file");
              }
              setInputFile(inputFile);
            }}
          >
            Select
          </Button>
        </div>
      </Field>

      <Field label="Output directory">
        <div
          css={{
            display: "flex",
            flexDirection: "row",
          }}
        >
          <Input
            css={{ flexGrow: 1 }}
            id={outputDirId}
            onChange={(e) => setOutputDir(e.currentTarget.value)}
            value={outputDir}
          />
          <Button
            appearance="outline"
            onClick={async () => {
              const outputDir = await open({
                directory: true,
              });
              if (outputDir === null) {
                return;
              }
              if (Array.isArray(outputDir)) {
                throw new Error("expected a single directory");
              }
              setOutputDir(outputDir);
            }}
          >
            Select
          </Button>
        </div>
      </Field>

      <Divider appearance="strong" />

      <Field validationMessage="Extracting..." validationState="none">
        <ProgressBar value={0.5} />
      </Field>

      <Button
        appearance="primary"
        disabled={
          inputFile.trim().length === 0 || outputDir.trim().length === 0
        }
        onClick={() => {
          invoke("extract", {
            payloadFile: inputFile,
            outputDir: outputDir,
          });
        }}
      >
        Extract
      </Button>
    </FluentProvider>
  );
}

export default App;

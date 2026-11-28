import { useState } from "react";
import { Button, Form, Input, Layout, Menu, Space, Table, Tag } from "antd";
import { text } from "@/strings";

const { Header, Sider, Content } = Layout;

type Row = {
  id: number;
  title: string;
  done: boolean;
};

const initialRows: Row[] = [
  { id: 1, title: "First task", done: false },
  { id: 2, title: "Second task", done: true },
];

export function App() {
  const [rows, setRows] = useState<Row[]>(initialRows);
  const [title, setTitle] = useState("");
  const [selected, setSelected] = useState("tasks");

  const create = () => {
    const value = title.trim();
    if (!value) return;
    setRows((current) => [
      ...current,
      { id: current.length + 1, title: value, done: false },
    ]);
    setTitle("");
  };

  return (
    <Layout style={{ minHeight: "100vh" }}>
      <Sider theme="light" width={220}>
        <div style={{ padding: 16, fontWeight: 600 }}>{text("brand")}</div>
        <Menu
          selectedKeys={[selected]}
          onClick={({ key }) => setSelected(key)}
          items={[{ key: "tasks", label: text("workspace") }]}
        />
      </Sider>
      <Layout>
        <Header
          style={{
            background: "#fff",
            display: "flex",
            alignItems: "center",
            gap: 12,
          }}
        >
          <strong>{text("studio")}</strong>
          <Tag color="green">{selected}</Tag>
        </Header>
        <Content style={{ padding: 24 }}>
          <Space direction="vertical" size="middle" style={{ width: "100%" }}>
            <Form layout="inline" onFinish={create}>
              <Form.Item>
                <Input
                  value={title}
                  onChange={(event) => setTitle(event.target.value)}
                  placeholder="New task"
                  aria-label="New task title"
                />
              </Form.Item>
              <Form.Item>
                <Button type="primary" htmlType="submit">
                  Create
                </Button>
              </Form.Item>
            </Form>
            <Table
              rowKey="id"
              dataSource={rows}
              columns={[
                { title: "ID", dataIndex: "id" },
                { title: "Title", dataIndex: "title" },
                {
                  title: "Done",
                  dataIndex: "done",
                  render: (done: boolean) => (
                    <Tag color={done ? "green" : "default"}>
                      {done ? "yes" : "no"}
                    </Tag>
                  ),
                },
              ]}
            />
          </Space>
        </Content>
      </Layout>
    </Layout>
  );
}